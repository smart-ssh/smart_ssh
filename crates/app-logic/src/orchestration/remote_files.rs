//! SFTP-Dateizugriff (`ReadRemoteFile`/`WriteRemoteFile`, Spec 0020) — s.
//! Moduldoc in `orchestration.rs` für den vollständigen Kontext (Spec
//! 0083: reine Verschiebung aus `orchestration.rs`, keine
//! Verhaltensänderung).

use uuid::Uuid;

use super::approval::ApprovedFileWrite;

use ssh_manager_core::ai::{fence_untrusted, ChatMessage, MessageContent, Role, UntrustedKind};
use ssh_manager_core::audit::{LedgerEntryContent, LedgerFileWriteStatus, LedgerSource};
use ssh_manager_core::profiles::{AiAction, ProfileStore};
use ssh_manager_core::ssh::elevated::shell_single_quote;
use ssh_manager_core::ssh::{CommandOutput, SshError};

use crate::events::{emit_chat_action_result, ActionResultPayload, EventEmitter};
use crate::session::Session;
use crate::state::{ActionId, SessionId};

use super::action_exec::{check_for_injected_instructions, emit_action_error};
use super::chat_turn::{push_history_scoped, write_ledger_entry_for_action};

// --- Spec 0020: SFTP-Dateizugriff (ReadRemoteFile/WriteRemoteFile) --------

/// Spec 0020, Abschnitt 4.1: Default-Obergrenze für `ReadRemoteFile` —
/// größere Dateien werden mit klarer Meldung abgelehnt statt vollständig in
/// den KI-Kontext geladen. Aktuell nicht nutzerkonfigurierbar (keine
/// entsprechende Einstellungs-UI vorgesehen).
pub(crate) const MAX_READ_FILE_BYTES: u64 = 256 * 1024;

/// Lowercase hex SHA-256 of `bytes` — the content fingerprint the activity
/// record keeps instead of the content itself (Spec 0057).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What a successful write leaves for the activity record. `sha256_before`
/// is `None` when the old content was not read (new file, or only readable
/// with elevated rights).
struct WriteDone {
    backup_path: Option<String>,
    sha256_before: Option<String>,
}

/// A failed write. `started` is true once the write to the target itself was
/// issued (direct SFTP write, or the privileged `install`), so the target may
/// be partly changed; failures before that (backup, temp file, permission
/// denied on open) leave it untouched. `backup_path` is set once the backup
/// exists.
struct WriteFailure {
    error: SshError,
    started: bool,
    backup_path: Option<String>,
}

impl WriteFailure {
    fn before_target(error: SshError) -> Self {
        Self {
            error,
            started: false,
            backup_path: None,
        }
    }
}

impl From<SshError> for WriteFailure {
    fn from(error: SshError) -> Self {
        Self::before_target(error)
    }
}

/// Spec 0020, Abschnitt 3: öffnet die SFTP-Session der Session lazy (erst
/// beim ersten Aufruf) und hält sie danach für die Dauer der Session offen
/// (`session.sftp` bleibt `Some`, bis die Session selbst endet). Ein
/// erneuter Aufruf, während bereits eine offene Session vorliegt, ist ein
/// No-op. `pub(crate)`, nicht privat: der manuelle Dateibrowser (Spec 0020,
/// Abschnitt 5, `app_shell::commands::sftp_*`) braucht dieselbe Lazy-Open-Logik,
/// läuft aber komplett außerhalb der KI-Kernschleife dieser Datei.
pub async fn ensure_sftp_open(session: &Session) -> Result<(), SshError> {
    // Lock hierarchy (docs/architecture.md, "Lock hierarchy of a session"):
    // SFTP channel first, transport second — only `SessionLocks` offers it.
    let mut locks = session.locks();
    let (mut guard, transport) = locks.sftp_then_transport_if_closed().await;
    if let Some(mut transport) = transport {
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

/// Spec 0088, A3.2: Ein leerer normaler SFTP-Kanal wird an genau einer
/// Stelle zum Fehler ([`crate::session::NormalSftpGuard::sftp_or_err`]).
/// Diese Funktion macht daraus einen sichtbaren Aktionsfehler im Chat, mit
/// Fehlercode und ohne Panic — für die beiden `stat`-Aufrufe, deren Ergebnis
/// sonst keinen Fehlerkanal hätte.
///
/// Nicht jeder Aufrufer braucht sie (spec-reviewer-Nachtrag, Runde 2): Die
/// beiden Schreib-Hilfsfunktionen liefern bereits `Result<_, SshError>` und
/// reichen den Fehler per `?` weiter, und am Lesepfad fällt er in den
/// bestehenden `Err`-Zweig von `read_file` (dieselbe Behandlung, nur mit
/// dessen Wortlaut). Gemeinsam ist allen: Der Fehler wird gemeldet,
/// **nachdem** die SFTP-Sperre gefallen ist — `emit_action_error` nimmt
/// `session.context`, und die Reihenfolge sftp→context soll hier gar nicht
/// erst entstehen.
async fn emit_sftp_channel_error(
    session: &Session,
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    path: &str,
    err: SshError,
    persist: bool,
) -> bool {
    let code = err.code();
    emit_action_error(
        session,
        emitter,
        session_id,
        format!("SFTP-Zugriff auf '{path}' nicht möglich: {err}"),
        Some(code),
        persist,
    )
    .await
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
    ledger_source: LedgerSource,
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
        // Spec 0088, A3.2: `sftp_or_err` statt `expect` — wäre der Kanal
        // wider Erwarten leer, endet die Aktion mit einem Fehler im Chat
        // statt mit einem Panic mitten im Chat-Turn.
        //
        // spec-reviewer-Fund (Runde 1): Der Fehler wird hier nur
        // herausgereicht und erst NACH diesem Block gemeldet. Sonst liefe
        // `emit_action_error` (nimmt `session.context`) unter der noch
        // (docs/architecture.md, "Lock hierarchy of a session")
        // gehaltenen SFTP-Sperre — eine Sperrreihenfolge sftp→context, die
        // es an dieser Stelle vorher nicht gab.
        match guard.sftp_or_err() {
            Ok(sftp) => Ok(sftp.stat(&path).await.map(|entry| entry.size).ok()),
            Err(err) => Err(err),
        }
    };
    let size = match size {
        Ok(size) => size,
        Err(err) => {
            return emit_sftp_channel_error(session, emitter, session_id, &path, err, persist).await
        }
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
        match guard.sftp_or_err() {
            Ok(sftp) => sftp.read_file(&path).await,
            // Derselbe Fehlertyp wie `read_file` — der leere Kanal läuft
            // damit durch den bestehenden `Err`-Zweig unten, ohne dass hier
            // unter gehaltener SFTP-Sperre etwas emittiert wird.
            Err(err) => Err(err),
        }
    };

    match raw {
        Ok(bytes) => {
            // Activity record (Spec 0057): path, size and hash only — never
            // the content. Hash of the raw bytes, before redaction.
            write_ledger_entry_for_action(
                session,
                ledger_source,
                LedgerEntryContent::FileRead {
                    path: path.clone(),
                    bytes: Some(bytes.len() as u64),
                    sha256: Some(sha256_hex(&bytes)),
                },
                Some(action_id),
            )
            .await;
            let redacted = ssh_manager_core::ai::OutputRedactor::redact(
                &session.redactor,
                &CommandOutput {
                    stdout: bytes,
                    stderr: Vec::new(),
                    exit_code: Some(0),
                    truncated: false,
                },
            );
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
                .safety
                .untrusted_content_ingested
                .store(true, std::sync::atomic::Ordering::SeqCst);
            check_for_injected_instructions(
                session,
                session_id,
                emitter,
                action_id,
                &content,
                "file_read",
            )
            .await;
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
) -> Result<WriteDone, WriteFailure> {
    let backup_path = if existed {
        Some(backup_path_for(path))
    } else {
        None
    };

    let mut sha256_before = None;
    if let Some(backup) = &backup_path {
        let mut guard = session.lock_sftp().await;
        let sftp = guard.sftp_or_err()?;
        let old_content = sftp.read_file(path).await?;
        sha256_before = Some(sha256_hex(&old_content));
        sftp.write_file(backup, &old_content).await?;
    }

    let mut guard = session.lock_sftp().await;
    let sftp = guard.sftp_or_err()?;
    if let Err(error) = sftp.write_file(path, content.as_bytes()).await {
        // Permission denied is refused on open: the target is untouched (and
        // the sudo fallback may follow). Any other error may be mid-stream.
        let started = !matches!(error, SshError::SftpPermissionDenied(_));
        return Err(WriteFailure {
            error,
            started,
            backup_path,
        });
    }

    Ok(WriteDone {
        backup_path,
        sha256_before,
    })
}

/// Führt `command` mit dem hinterlegten Sudo-Passwort über Stdin aus (Spec
/// 0018, Abschnitt 5) und wertet den Exit-Code aus — anders als
/// `execute_suggested_command` (das den rohen Output unabhängig vom
/// Exit-Code als Kommando-Ergebnis zurückgibt) braucht dieser interne
/// Aufbauschritt ein hartes Erfolg/Fehlschlag-Signal.
async fn execute_privileged(
    // Proof that a confirmed `WriteRemoteFile` action leads here.
    _approval: &ApprovedFileWrite,
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
    approval: &ApprovedFileWrite,
    session: &Session,
    path: &str,
    content: &str,
    existed: bool,
    old_mode: Option<u32>,
    password: &secrecy::SecretString,
) -> Result<WriteDone, WriteFailure> {
    let backup_path = if existed {
        Some(backup_path_for(path))
    } else {
        None
    };

    // "Before" hash: only when the old content is readable over SFTP (e.g. a
    // 0644 root file); `None` when the read itself is denied or fails.
    let mut sha256_before = None;
    if existed {
        let mut guard = session.lock_sftp().await;
        if let Some(sftp) = guard.sftp() {
            if let Ok(old) = sftp.read_file(path).await {
                sha256_before = Some(sha256_hex(&old));
            }
        }
    }

    if let Some(backup) = &backup_path {
        let cmd = format!(
            "sudo -S cp -p {} {}",
            shell_single_quote(path),
            shell_single_quote(backup)
        );
        execute_privileged(approval, session, &cmd, password).await?;
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
        let sftp = guard.sftp_or_err()?;
        sftp.write_file(&temp_name, content.as_bytes())
            .await
            .map_err(|e| {
                SshError::ChannelError(format!("Temp-Datei konnte nicht angelegt werden: {e}"))
            })?;
    }

    let install_cmd = format!(
        "sudo -S install -m {mode:o} {} {}",
        shell_single_quote(&temp_name),
        shell_single_quote(path)
    );
    let install_result = execute_privileged(approval, session, &install_cmd, password).await;

    // Temp-Datei aufräumen, unabhängig vom Ergebnis des `install`-Aufrufs.
    {
        let mut guard = session.lock_sftp().await;
        if let Some(sftp) = guard.sftp() {
            let _ = sftp.remove(&temp_name).await;
        }
    }

    // The `install` command is the write to the target; a non-zero exit may
    // come after it began copying, so the target counts as possibly changed.
    if let Err(error) = install_result {
        return Err(WriteFailure {
            error,
            started: true,
            backup_path,
        });
    }
    Ok(WriteDone {
        backup_path,
        sha256_before,
    })
}

/// Activity record for a write that failed after it started (the target may be
/// partly overwritten). Writes nothing when the target was never touched.
/// No content, no hashes: nothing is known to have been written.
#[expect(clippy::too_many_arguments, reason = "one argument per input")]
async fn record_failed_write(
    session: &Session,
    ledger_source: LedgerSource,
    path: &str,
    bytes_before: Option<u64>,
    existed: bool,
    failure: &WriteFailure,
    sudo: bool,
    action_id: Uuid,
) {
    if !failure.started {
        return;
    }
    // A new file whose open failed (missing directory, wrong sudo password)
    // was never touched: if the stat says it is missing, nothing was written
    // and nothing is recorded. If the state cannot be established (session
    // closed, permission denied, no channel) the file may be partly written,
    // so the failed entry is kept.
    if !existed {
        let mut guard = session.lock_sftp().await;
        if let Some(sftp) = guard.sftp() {
            match sftp.stat(path).await {
                Ok(_)
                | Err(
                    ssh_manager_core::ssh::SshError::SessionClosed(_)
                    | ssh_manager_core::ssh::SshError::SftpPermissionDenied(_),
                ) => {}
                Err(_) => return,
            }
        }
    }
    write_ledger_entry_for_action(
        session,
        ledger_source,
        LedgerEntryContent::FileWritten {
            path: path.to_string(),
            status: LedgerFileWriteStatus::Failed,
            backup_path: failure.backup_path.clone(),
            bytes_before,
            bytes_after: None,
            sha256_before: None,
            sha256_after: None,
            sudo,
        },
        Some(action_id),
    )
    .await;
}

/// Spec 0020, Abschnitt 4.2/4.3: kompletter Schreib-Ablauf — regulärer
/// SFTP-Versuch zuerst, bei fehlenden Rechten (und **nur** dann) Sudo-
/// Fallback, sofern für den Server ein Passwort hinterlegt ist. Ohne
/// Passwort wird der ursprüngliche Fehler unverändert gemeldet (Abschnitt
/// 4.3, Punkt 5: "kein stiller Fallback").
#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one argument per input; one linear write flow"
)]
pub(crate) async fn execute_write_remote_file(
    session: &Session,
    session_id: SessionId,
    action_id: ActionId,
    approved: ApprovedFileWrite,
    emitter: &dyn EventEmitter,
    profile_store: &dyn ProfileStore,
    persist: bool,
    sudo_fallback_announced: bool,
    ledger_source: LedgerSource,
) -> bool {
    let path = approved.path().to_owned();
    let content = approved.content().to_owned();
    // Issue #464 (Spec 0038): the edition's transform runs on the final
    // approved content, right before it is written. Result, history and
    // errors below never see `wire_content`.
    let prepared = match super::outgoing::prepare_for_send(
        session,
        profile_store,
        ssh_manager_core::outgoing::OutgoingKind::FileContent,
        &content,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(message) => {
            return emit_action_error(session, emitter, session_id, message, None, persist).await;
        }
    };
    let wire_content: &str = match &prepared {
        Some(text) => {
            use secrecy::ExposeSecret;
            text.expose_secret()
        }
        None => content.as_str(),
    };

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

    let stat_result = {
        let mut guard = session.lock_sftp().await;
        match guard.sftp_or_err() {
            Ok(sftp) => Ok(match sftp.stat(&path).await {
                Ok(entry) => (true, Some(entry.permissions), Some(entry.size)),
                Err(_) => (false, None, None),
            }),
            // s. `execute_read_remote_file`: melden erst nach dem Block.
            Err(err) => Err(err),
        }
    };
    let (existed, old_mode, bytes_before) = match stat_result {
        Ok(found) => found,
        Err(err) => {
            return emit_sftp_channel_error(session, emitter, session_id, &path, err, persist).await
        }
    };

    let regular = write_via_sftp_with_backup(session, &path, wire_content, existed).await;

    let (done, used_sudo_password) = match regular {
        Ok(done) => (done, false),
        Err(WriteFailure {
            error: SshError::SftpPermissionDenied(_),
            ..
        }) => {
            // Spec 0068, Teil 3: Sudo-Fallback nur, wenn der Dialog ihn vorab
            // angekündigt hat — genau der im Event gesendete Wert
            // (Review-Fund: keine Neuberechnung, die auseinanderlaufen kann).
            let Some(password) = session
                .safety
                .sudo_password
                .get()
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
            match write_via_sudo_fallback(
                &approved,
                session,
                &path,
                wire_content,
                existed,
                old_mode,
                &password,
            )
            .await
            {
                Ok(done) => (done, true),
                Err(failure) => {
                    record_failed_write(
                        session,
                        ledger_source,
                        &path,
                        bytes_before,
                        existed,
                        &failure,
                        true,
                        action_id,
                    )
                    .await;
                    let err = failure.error;
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
        Err(failure) => {
            record_failed_write(
                session,
                ledger_source,
                &path,
                bytes_before,
                existed,
                &failure,
                false,
                action_id,
            )
            .await;
            let err = failure.error;
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

    let WriteDone {
        backup_path,
        sha256_before,
    } = done;
    // Activity record (Spec 0057): written only now, when bytes were written.
    // The hash is of the bytes that went over the wire; no content, no diff.
    write_ledger_entry_for_action(
        session,
        ledger_source,
        LedgerEntryContent::FileWritten {
            path: path.clone(),
            status: LedgerFileWriteStatus::Completed,
            backup_path: backup_path.clone(),
            bytes_before,
            bytes_after: Some(wire_content.len() as u64),
            sha256_before,
            sha256_after: Some(sha256_hex(wire_content.as_bytes())),
            sudo: used_sudo_password,
        },
        Some(action_id),
    )
    .await;
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
