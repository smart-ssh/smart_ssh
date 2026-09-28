//! SFTP-Dateizugriff (`ReadRemoteFile`/`WriteRemoteFile`, Spec 0020) — s.
//! Moduldoc in `orchestration.rs` für den vollständigen Kontext (Spec
//! 0083: reine Verschiebung aus `orchestration.rs`, keine
//! Verhaltensänderung).

use uuid::Uuid;

use ssh_manager_core::ai::{fence_untrusted, ChatMessage, MessageContent, Role, UntrustedKind};
use ssh_manager_core::profiles::AiAction;
use ssh_manager_core::ssh::{CommandOutput, SshError};

use crate::events::{emit_chat_action_result, ActionResultPayload, EventEmitter};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::action_exec::{check_for_injected_instructions, emit_action_error};
use super::chat_turn::push_history_scoped;

// --- Spec 0020: SFTP-Dateizugriff (ReadRemoteFile/WriteRemoteFile) --------

/// Spec 0020, Abschnitt 4.1: Default-Obergrenze für `ReadRemoteFile` —
/// größere Dateien werden mit klarer Meldung abgelehnt statt vollständig in
/// den KI-Kontext geladen. Aktuell nicht nutzerkonfigurierbar (keine
/// entsprechende Einstellungs-UI vorgesehen).
pub(crate) const MAX_READ_FILE_BYTES: u64 = 256 * 1024;

/// Spec 0020, Abschnitt 3: öffnet die SFTP-Session der Session lazy (erst
/// beim ersten Aufruf) und hält sie danach für die Dauer der Session offen
/// (`session.sftp` bleibt `Some`, bis die Session selbst endet). Ein
/// erneuter Aufruf, während bereits eine offene Session vorliegt, ist ein
/// No-op. `pub(crate)`, nicht privat: der manuelle Dateibrowser (Spec 0020,
/// Abschnitt 5, `crate::commands::sftp_*`) braucht dieselbe Lazy-Open-Logik,
/// läuft aber komplett außerhalb der KI-Kernschleife dieser Datei.
pub async fn ensure_sftp_open(session: &Session) -> Result<(), SshError> {
    let mut guard = session.lock_sftp().await;
    if !guard.is_open() {
        let mut transport = session.transport.lock().await;
        let sftp = transport.open_sftp().await?;
        // Spec 0085, A3.2: die eine Stelle, die den normalen Kanal befüllt —
        // und sie nimmt ihn aus dem Transport dieser Sitzung selbst.
        guard.install(sftp);
    }
    Ok(())
}

/// Spec 0020, Abschnitt 4.2, Punkt 3: liest die aktuelle Zieldatei einer
/// `WriteRemoteFile`-Aktion (falls vorhanden) für die Diff-Vorschau im
/// Bestätigungsdialog. `(None, None)` für alle anderen Aktionstypen sowie
/// wenn die Datei nicht existiert oder SFTP aus einem anderen Grund gerade
/// nicht verfügbar ist (kein harter Fehler an dieser Stelle — die Vorschau
/// ist eine Zusatzinformation, kein Blocker für den Vorschlag selbst).
/// `(Some(text), None)` bei einer als UTF-8 dekodierbaren bestehenden
/// Datei; `(None, Some(size))` bei einer bestehenden Binärdatei (Abschnitt
/// 4.2, Punkt 3, letzter Satz).
pub(crate) async fn previous_file_content_for_action(
    action: &AiAction,
    session: &Session,
) -> (Option<String>, Option<u64>) {
    let AiAction::WriteRemoteFile { path, .. } = action else {
        return (None, None);
    };
    if ensure_sftp_open(session).await.is_err() {
        return (None, None);
    }
    let mut guard = session.lock_sftp().await;
    let Some(sftp) = guard.sftp() else {
        return (None, None);
    };
    match sftp.read_file(path).await {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => (Some(text), None),
            Err(err) => (None, Some(err.into_bytes().len() as u64)),
        },
        Err(_) => (None, None),
    }
}

/// Spec 0020, Abschnitt 4.1: liest die Datei per SFTP, lehnt sie über
/// `MAX_READ_FILE_BYTES` mit klarer Meldung ab statt sie zu laden, läuft
/// sonst durch denselben `OutputRedactor` wie Kommando-Output (Spec 0006,
/// Abschnitt 5) — als `CommandOutput` mit leerem `stderr` "verpackt", um die
/// bestehende Redactor-Schnittstelle wiederzuverwenden, statt eine zweite,
/// nur für Dateiinhalte zuständige Methode einzuführen.
pub(crate) async fn execute_read_remote_file(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    path: String,
    emitter: &dyn EventEmitter,
    persist: bool,
) -> bool {
    if let Err(err) = ensure_sftp_open(session).await {
        let code = err.code();
        return emit_action_error(
            session,
            emitter,
            session_id,
            format!("SFTP konnte nicht geöffnet werden: {err}"),
            Some(code),
            persist,
        )
        .await;
    }

    // Größenprüfung vor dem eigentlichen Lesen — ein fehlgeschlagenes
    // `stat()` blockiert `read_file` selbst nicht (manche Server/Pfade
    // könnten `stat` anders behandeln als `read`), die Prüfung wird dann
    // schlicht übersprungen statt den ganzen Aufruf scheitern zu lassen.
    let size = {
        let mut guard = session.lock_sftp().await;
        let sftp = guard
            .sftp()
            .expect("ensure_sftp_open lief erfolgreich durch");
        sftp.stat(&path).await.map(|entry| entry.size).ok()
    };
    if let Some(size) = size {
        if size > MAX_READ_FILE_BYTES {
            return emit_action_error(
                session,
                emitter,
                session_id,
                format!(
                    "Datei '{path}' ist zu groß ({size} Bytes, Obergrenze \
                     {MAX_READ_FILE_BYTES} Bytes) — wird nicht gelesen."
                ),
                None,
                persist,
            )
            .await;
        }
    }

    let raw = {
        let mut guard = session.lock_sftp().await;
        let sftp = guard
            .sftp()
            .expect("ensure_sftp_open lief erfolgreich durch");
        sftp.read_file(&path).await
    };

    match raw {
        Ok(bytes) => {
            let redacted = session.redactor.redact(&CommandOutput {
                stdout: bytes,
                stderr: Vec::new(),
                exit_code: Some(0),
                truncated: false,
            });
            let content = String::from_utf8_lossy(&redacted.stdout).into_owned();
            emit_chat_action_result(
                emitter,
                session_id,
                action_id,
                ActionResultPayload::FileRead {
                    path: path.clone(),
                    content: content.clone(),
                },
            );
            // Spec 0039, Abschnitt 3: SFTP-Dateiinhalt ging bisher als
            // normale, ungefencte User-Nachricht in den Kontext — für das
            // Modell nicht von etwas unterscheidbar, das der Nutzer selbst
            // getippt hat. `fence_untrusted` markiert ihn jetzt eindeutig
            // als Daten aus einer nicht vertrauenswürdigen Quelle. Die
            // Live-UI-Karte (`ActionResultPayload::FileRead` oben) zeigt
            // bewusst weiter den unformatierten Inhalt — das Fencing ist
            // nur für den KI-Kontext relevant, nicht für die Anzeige.
            push_history_scoped(
                session,
                ChatMessage {
                    role: Role::ActionResult,
                    content: MessageContent::Text(format!(
                        "Inhalt von '{path}':\n\n{}",
                        fence_untrusted(UntrustedKind::RemoteFile, &path, &content)
                    )),
                },
                persist,
            )
            .await;
            // Spec 0039, Abschnitt 5.
            session
                .untrusted_content_ingested
                .store(true, std::sync::atomic::Ordering::SeqCst);
            check_for_injected_instructions(session, session_id, emitter, &content).await;
            true
        }
        Err(err) => {
            let code = err.code();
            emit_action_error(
                session,
                emitter,
                session_id,
                format!("Lesen von '{path}' fehlgeschlagen: {err}"),
                Some(code),
                persist,
            )
            .await
        }
    }
}

fn backup_path_for(path: &str) -> String {
    format!(
        "{path}.smartssh-backup-{}",
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    )
}

/// Einfaches POSIX-Single-Quote-Escaping für Pfade, die als Argument in ein
/// per `execute_with_stdin` ausgeführtes Shell-Kommando eingebettet werden
/// (Spec 0020, Abschnitt 4.3).
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Regulärer (nicht-privilegierter) Schreibversuch per SFTP: Backup (falls
/// die Datei existiert) + eigentliches Schreiben, in dieser Reihenfolge
/// (Spec 0020, Abschnitt 4.2, Punkt 4: "vor jedem Überschreiben"). Gibt den
/// Backup-Pfad zurück, falls einer angelegt wurde. Ein
/// `SshError::SftpPermissionDenied` an beliebiger Stelle signalisiert dem
/// Aufrufer, dass Abschnitt 4.3 (Sudo-Rechte-Fallback) greifen sollte.
async fn write_via_sftp_with_backup(
    session: &Session,
    path: &str,
    content: &str,
    existed: bool,
) -> Result<Option<String>, SshError> {
    let backup_path = if existed {
        Some(backup_path_for(path))
    } else {
        None
    };

    if let Some(backup) = &backup_path {
        let mut guard = session.lock_sftp().await;
        let sftp = guard
            .sftp()
            .expect("ensure_sftp_open lief erfolgreich durch");
        let old_content = sftp.read_file(path).await?;
        sftp.write_file(backup, &old_content).await?;
    }

    let mut guard = session.lock_sftp().await;
    let sftp = guard
        .sftp()
        .expect("ensure_sftp_open lief erfolgreich durch");
    sftp.write_file(path, content.as_bytes()).await?;

    Ok(backup_path)
}

/// Führt `command` mit dem hinterlegten Sudo-Passwort über Stdin aus (Spec
/// 0018, Abschnitt 5) und wertet den Exit-Code aus — anders als
/// `execute_suggested_command` (das den rohen Output unabhängig vom
/// Exit-Code als Kommando-Ergebnis zurückgibt) braucht dieser interne
/// Aufbauschritt ein hartes Erfolg/Fehlschlag-Signal.
async fn execute_privileged(
    session: &Session,
    command: &str,
    password: &secrecy::SecretString,
) -> Result<(), SshError> {
    use secrecy::ExposeSecret;
    let mut stdin = password.expose_secret().as_bytes().to_vec();
    stdin.push(b'\n');
    let output = {
        let mut transport = session.transport.lock().await;
        transport.execute_with_stdin(command, &stdin).await?
    };
    if output.exit_code == Some(0) {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(SshError::ChannelError(format!(
            "Kommando fehlgeschlagen (exit {:?}): {stderr}",
            output.exit_code
        )))
    }
}

/// Spec 0020, Abschnitt 4.3: Sudo-Rechte-Fallback, nachdem der reguläre
/// SFTP-Schreibversuch mit `SftpPermissionDenied` gescheitert ist. Das
/// Backup (falls die Datei existiert) läuft hier ebenfalls privilegiert
/// (`sudo -S cp -p`, Punkt 4) statt per SFTP-Lesen+Schreiben — ein erneuter
/// SFTP-Lesevesuch würde mit derselben Rechte-Einschränkung scheitern wie
/// der ursprüngliche Schreibversuch, SFTP kennt zudem kein eigenes
/// "Kopieren".
async fn write_via_sudo_fallback(
    session: &Session,
    path: &str,
    content: &str,
    existed: bool,
    old_mode: Option<u32>,
    password: &secrecy::SecretString,
) -> Result<Option<String>, SshError> {
    let backup_path = if existed {
        Some(backup_path_for(path))
    } else {
        None
    };

    if let Some(backup) = &backup_path {
        let cmd = format!(
            "sudo -S cp -p {} {}",
            shell_quote(path),
            shell_quote(backup)
        );
        execute_privileged(session, &cmd, password).await?;
    }

    // `install -m` statt `mv`, um Rechte/Eigentümer des Ziels in einem
    // Schritt korrekt zu setzen, statt sie vom Temp-File zu erben (Spec
    // 0020, Abschnitt 4.3, Punkt 3) — Default 0o644 für eine neue Datei
    // ohne bekannten alten Modus.
    let mode = old_mode.unwrap_or(0o644) & 0o7777;

    // Temp-Datei im Home-Verzeichnis des Login-Users über SFTP schreiben —
    // relativer Pfad (kein führender `/`), SFTP-Server lösen relative
    // Pfade konventionell relativ zum Home-Verzeichnis auf.
    let temp_name = format!(".smartssh-tmp-{}", Uuid::new_v4());
    {
        let mut guard = session.lock_sftp().await;
        let sftp = guard
            .sftp()
            .expect("ensure_sftp_open lief erfolgreich durch");
        sftp.write_file(&temp_name, content.as_bytes())
            .await
            .map_err(|e| {
                SshError::ChannelError(format!("Temp-Datei konnte nicht angelegt werden: {e}"))
            })?;
    }

    let install_cmd = format!(
        "sudo -S install -m {mode:o} {} {}",
        shell_quote(&temp_name),
        shell_quote(path)
    );
    let install_result = execute_privileged(session, &install_cmd, password).await;

    // Temp-Datei aufräumen, unabhängig vom Ergebnis des `install`-Aufrufs.
    {
        let mut guard = session.lock_sftp().await;
        if let Some(sftp) = guard.sftp() {
            let _ = sftp.remove(&temp_name).await;
        }
    }

    install_result?;
    Ok(backup_path)
}

/// Spec 0020, Abschnitt 4.2/4.3: kompletter Schreib-Ablauf — regulärer
/// SFTP-Versuch zuerst, bei fehlenden Rechten (und **nur** dann) Sudo-
/// Fallback, sofern für den Server ein Passwort hinterlegt ist. Ohne
/// Passwort wird der ursprüngliche Fehler unverändert gemeldet (Abschnitt
/// 4.3, Punkt 5: "kein stiller Fallback").
#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_write_remote_file(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    path: String,
    content: String,
    emitter: &dyn EventEmitter,
    persist: bool,
    sudo_fallback_announced: bool,
) -> bool {
    if let Err(err) = ensure_sftp_open(session).await {
        let code = err.code();
        return emit_action_error(
            session,
            emitter,
            session_id,
            format!("SFTP konnte nicht geöffnet werden: {err}"),
            Some(code),
            persist,
        )
        .await;
    }

    let (existed, old_mode) = {
        let mut guard = session.lock_sftp().await;
        let sftp = guard
            .sftp()
            .expect("ensure_sftp_open lief erfolgreich durch");
        match sftp.stat(&path).await {
            Ok(entry) => (true, Some(entry.permissions)),
            Err(_) => (false, None),
        }
    };

    let regular = write_via_sftp_with_backup(session, &path, &content, existed).await;

    let (backup_path, used_sudo_password) = match regular {
        Ok(backup_path) => (backup_path, false),
        Err(SshError::SftpPermissionDenied(_)) => {
            // Spec 0068, Teil 3: Sudo-Fallback nur, wenn der Dialog ihn vorab
            // angekündigt hat — genau der im Event gesendete Wert
            // (Review-Fund: keine Neuberechnung, die auseinanderlaufen kann).
            let Some(password) = session
                .sudo_password
                .clone()
                .filter(|_| sudo_fallback_announced)
            else {
                return emit_action_error(
                    session,
                    emitter,
                    session_id,
                    format!(
                        "Zugriff verweigert beim Schreiben von '{path}' — erhöhte Rechte nötig, \
                         aber kein Sudo-Passwort für diesen Server hinterlegt."
                    ),
                    // Kein Sudo-Passwort hinterlegt — derselbe zugrundeliegende
                    // Fehlerfall (`SftpPermissionDenied`) wie der Err-Zweig oben,
                    // hier aber ohne Passwort-Fallback-Versuch abgefangen, bevor
                    // ein neuer `SshError`-Wert entstünde.
                    Some(SshError::SftpPermissionDenied(String::new()).code()),
                    persist,
                )
                .await;
            };
            match write_via_sudo_fallback(session, &path, &content, existed, old_mode, &password)
                .await
            {
                Ok(backup_path) => (backup_path, true),
                Err(err) => {
                    let code = err.code();
                    return emit_action_error(
                        session,
                        emitter,
                        session_id,
                        format!(
                            "Schreiben von '{path}' fehlgeschlagen (auch mit Sudo-Rechten): {err}"
                        ),
                        Some(code),
                        persist,
                    )
                    .await;
                }
            }
        }
        Err(err) => {
            let code = err.code();
            return emit_action_error(
                session,
                emitter,
                session_id,
                format!("Schreiben von '{path}' fehlgeschlagen: {err}"),
                Some(code),
                persist,
            )
            .await;
        }
    };

    let summary = match &backup_path {
        Some(backup) => format!("Datei '{path}' geschrieben (Backup: '{backup}')."),
        None => format!("Datei '{path}' neu angelegt."),
    };
    emit_chat_action_result(
        emitter,
        session_id,
        action_id,
        ActionResultPayload::FileWrite {
            path: path.clone(),
            backup_path,
            used_sudo_password,
        },
    );
    push_history_scoped(
        session,
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::Text(summary),
        },
        persist,
    )
    .await;
    true
}
