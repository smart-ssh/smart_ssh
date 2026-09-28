//! Spec 0020/0054/0067: manueller Dateibrowser (Auflisten, Download/Upload,
//! Löschen/chmod/Umbenennen, "Lokal öffnen") — Teil der Spec-0083-Aufteilung
//! von `commands.rs`.
//!
//! Bewusst OHNE Filter-Engine-Prüfung — anders als `ReadRemoteFile`/
//! `WriteRemoteFile` (Spec 0020, Abschnitt 4, `app_logic::orchestration`) laufen
//! diese Befehle nie über den KI-Chat, sondern sind direkte Nutzeraktionen im
//! Dateibrowser-Panel, analog zum interaktiven Terminal (Spec 0005, Abschnitt
//! 1: auch dort läuft rohe Tastatureingabe ungefiltert durch).
//!
//! Historische Anmerkung (Spec 0020, Teil 1): `remove()` (SFTP `REMOVE`)
//! wirkt nur auf Dateien, `sftp_download`/`sftp_delete` waren deshalb lange
//! auf Dateien beschränkt. Spec 0054 hebt das auf: `sftp_download_default`/
//! `sftp_download_dir` (Teil 2) laden Ordner rekursiv herunter, `sftp_delete`
//! (Teil 3, unten) löscht sie rekursiv über die neuen Trait-Methoden
//! `remove_dir`/das Zusammenspiel mit `list_dir`. "Umbenennen" (SFTP
//! `RENAME`, für beide Eintragstypen) war davon nie betroffen.

use tauri::{AppHandle, State};
use uuid::Uuid;

use ssh_manager_core::ssh::{SftpSession, SshError};

use crate::elevated_sftp::ElevatedSftpRegistry;
use crate::event_emitter::TauriEventEmitter;
use app_logic::dto::{sort_remote_entries, EditSessionDto, RemoteEntryDto};
use app_logic::error::CommandError;
use app_logic::error::CommandResult;
use app_logic::events::{
    emit_sftp_transfer_finished, emit_sftp_transfer_started, EventEmitter, SftpTransferKind,
};
use app_logic::session::Session;
use app_logic::state::{AppState, SessionId};

use super::elevation::{
    audit_elevated_change, file_name_of, lock_browser_sftp, run_browser_command,
    safe_local_segment, write_local_download, BrowserChannel,
};

/// Spec 0085, A1 (T1–T9): Widerruf des erhöhten Modus innerhalb eines schon
/// laufenden Browser-Befehls. Eigene Datei, weil der Aufbau (Test-Double mit
/// Haltepunkt, Audit-Mitschnitt) für sich genommen umfangreich ist.
#[cfg(test)]
mod revocation_tests;

/// Spec 0086, A1 (T1–T7): die Größengrenzen von „Dateiinhalt kopieren" und
/// „Lokal öffnen" halten auch bei einer zwischen `stat` und `read_file`
/// gewachsenen Datei. Eigene Datei, weil der Aufbau (Kanal-Double, bei dem
/// `stat` und `read_file` sich widersprechen) für sich genommen umfangreich
/// ist.
#[cfg(test)]
mod size_limit_tests;

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<Vec<RemoteEntryDto>> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { list_impl(&session, &channel, &path).await },
    )
    .await
}

async fn list_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<Vec<RemoteEntryDto>> {
    let entries = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        sftp.list_dir(path).await?
    };
    let mut dtos: Vec<RemoteEntryDto> = entries.iter().map(RemoteEntryDto::from).collect();
    sort_remote_entries(&mut dtos);
    Ok(dtos)
}

/// Lädt genau eine Remote-Datei nach `local_path` herunter — der
/// eigentliche Transfer-Kern hinter `sftp_download`, `sftp_download_default`
/// und `sftp_download_dir` (Ordner-Rekursion, s. `download_recursive`
/// unten): jeweils ein `sftp-transfer-started`/`-finished`-Ereignispaar (s.
/// `app_logic::events`-Moduldoc zur Fortschritts-Design-Entscheidung), dann
/// Lesen per SFTP + lokales Schreiben via `spawn_blocking` (Downloads können
/// beliebig groß sein, Spec 0020 Abschnitt 5 verlangt ausdrücklich, dass
/// Transfers die Session nicht blockieren).
/// Spec 0086, T8: `emitter` statt eines `AppHandle` — der Transfer-Kern baute
/// den Emitter bisher selbst, damit war er für keinen Test erreichbar und die
/// Meldung im Ereignis nirgends abgedeckt. `app_logic::events::EventEmitter`
/// ist dieselbe Abstraktion, die der Produktivpfad benutzt
/// (`TauriEventEmitter`), also keine Testsonderbahn.
async fn download_one_file(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_path: &str,
    local_path: std::path::PathBuf,
    total_bytes: Option<u64>,
) -> CommandResult<()> {
    let file_name = file_name_of(remote_path);
    let transfer_id = Uuid::new_v4();
    emit_sftp_transfer_started(
        emitter,
        session_id,
        transfer_id,
        SftpTransferKind::Download,
        file_name,
        total_bytes,
    );

    let result: CommandResult<()> = async {
        let bytes = {
            let mut guard = lock_browser_sftp(session, channel).await;
            let mut sftp = guard.sftp().await?;
            sftp.read_file(remote_path).await?
        };
        // Spec 0067, spec-reviewer-Fund: im erhöhten Modus können das
        // Root-Dateien sein (z. B. /etc/shadow) — lokal nur für den eigenen
        // Nutzer lesbar anlegen statt mit der Standard-umask.
        let restrict = channel.is_elevated();
        tokio::task::spawn_blocking(move || write_local_download(&local_path, &bytes, restrict))
            .await
            .map_err(|e| format!("Hintergrund-Task für Download fehlgeschlagen: {e}"))??;
        Ok(())
    }
    .await;

    emit_sftp_transfer_finished(
        emitter,
        session_id,
        transfer_id,
        // Spec 0086, A2.1: derselbe Text wie im Befehlsergebnis, wenn der
        // erhöhte Modus mitten im Transfer widerrufen wurde — s.
        // `BrowserChannel::transfer_error_message`.
        result
            .as_ref()
            .err()
            .map(|e| channel.transfer_error_message(e)),
    );
    result
}

/// Spec 0020, Abschnitt 5: "nativer Speichern-Dialog" — derselbe
/// oneshot-Kanal-Umweg wie `export_document` (dortiger Doc-Kommentar erklärt
/// das Warum). Datei-only (s. Moduldoc "Design-Entscheidung" oben) und
/// per Dialog an einen präzisen lokalen Zielpfad — Ordner-Download und
/// Download ohne Dialog sind `sftp_download_dir`/`sftp_download_default`
/// (Spec 0054, Teil 2) weiter unten.
#[tauri::command]
pub async fn sftp_download(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<Option<app_logic::dto::DownloadResultDto>> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            download_impl(&app, &session, &channel, session_id, &remote_path).await
        },
    )
    .await
}

async fn download_impl(
    app: &AppHandle,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_path: &str,
) -> CommandResult<Option<app_logic::dto::DownloadResultDto>> {
    use tauri_plugin_dialog::DialogExt;

    let file_name = file_name_of(remote_path);

    // Größe vorab für die Fortschrittsanzeige — ein fehlgeschlagenes
    // `stat()` (z. B. eingeschränkte Leserechte aufs Elternverzeichnis)
    // blockiert den eigentlichen Download nicht, die Anzeige zeigt dann
    // schlicht keine Gesamtgröße. Ein **Widerruf** an dieser Stelle wird
    // dadurch nicht verschluckt: er ist im Abbruch-Vermerk des Befehls
    // festgehalten und schlägt beim Abschluss durch (Spec 0085, A1.2).
    let total_bytes = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        sftp.stat(remote_path).await.ok().map(|entry| entry.size)
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(&file_name)
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(local_path) = rx.await.ok().flatten() else {
        return Ok(None); // Abbrechen ist kein Fehler, s. `export_document`.
    };
    let local_path = local_path.into_path()?;

    download_one_file(
        &TauriEventEmitter(app.clone()),
        session,
        channel,
        session_id,
        remote_path,
        local_path.clone(),
        total_bytes,
    )
    .await?;
    Ok(Some(app_logic::dto::DownloadResultDto {
        local_path: local_path.to_string_lossy().into_owned(),
        is_dir: false,
        file_count: 1,
    }))
}

/// Ermittelt das Standard-Downloadverzeichnis des Betriebssystems (Spec
/// 0054, Teil 2: "Standard-Downloadverzeichnis ODER präziser Pfad per
/// Dialog") — `directories::UserDirs` ist bereits Projektabhängigkeit (s.
/// `logging.rs`).
fn default_downloads_dir() -> CommandResult<std::path::PathBuf> {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(|p| p.to_path_buf()))
        .ok_or("Kein Standard-Downloadverzeichnis gefunden")
        .map_err(CommandError::from)
}

/// Rekursiver Ordner-Download (Spec 0054, Teil 2: "Ordner rekursiv"):
/// listet iterativ (kein async-rekursiver Aufruf nötig — vermeidet das
/// Boxing, das ein `async fn`, das sich selbst aufruft, in Rust braucht)
/// über eine Arbeits-Warteschlange, legt lokale Unterordner an und lädt
/// jede gefundene Datei einzeln über `download_one_file` — dadurch bekommt
/// jede Datei ihr eigenes `sftp-transfer-started`/`-finished`-Paar, die
/// Transfer-Liste im Frontend zeigt also automatisch den Fortschritt über
/// den ganzen Baum, ohne einen zweiten Fortschritts-Mechanismus.
async fn download_recursive(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_root: &str,
    local_root: &std::path::Path,
) -> CommandResult<u64> {
    tokio::fs::create_dir_all(local_root).await?;
    let mut file_count = 0u64;
    let mut queue = vec![(remote_root.to_string(), local_root.to_path_buf())];
    while let Some((remote_dir, local_dir)) = queue.pop() {
        let entries = {
            let mut guard = lock_browser_sftp(session, channel).await;
            let mut sftp = guard.sftp().await?;
            sftp.list_dir(&remote_dir).await?
        };
        for entry in entries {
            safe_local_segment(&entry.name)?;
            let local_entry_path = local_dir.join(&entry.name);
            if entry.is_dir {
                tokio::fs::create_dir_all(&local_entry_path).await?;
                queue.push((entry.path, local_entry_path));
            } else {
                download_one_file(
                    emitter,
                    session,
                    channel,
                    session_id,
                    &entry.path,
                    local_entry_path,
                    Some(entry.size),
                )
                .await?;
                file_count += 1;
            }
        }
    }
    Ok(file_count)
}

/// Lädt `remote_path` (Datei oder Ordner) unter `local_base_dir` herunter —
/// gemeinsame Logik von `sftp_download_default` und `sftp_download_dir`.
/// Eine Datei landet direkt als `local_base_dir/<dateiname>`, ein Ordner
/// als `local_base_dir/<ordnername>/...` (rekursiv) — nie werden die
/// Inhalte eines Ordners direkt lose in `local_base_dir` verstreut, das
/// bliebe sonst nicht als "der heruntergeladene Ordner" wiedererkennbar.
async fn download_entry_to(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_path: &str,
    local_base_dir: &std::path::Path,
) -> CommandResult<app_logic::dto::DownloadResultDto> {
    let root_name = file_name_of(remote_path);
    // `remote_path` kommt vom Frontend, letztlich aber aus einem früheren
    // `sftp_list`-Ergebnis (`RemoteEntryDto.path`) — also transitiv
    // server-kontrolliert. Dieselbe Zip-Slip-Prüfung wie in
    // `download_recursive` für jeden rekursiv entdeckten Eintrag.
    safe_local_segment(&root_name)?;
    let root_entry = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        sftp.stat(remote_path).await?
    };
    let local_path = local_base_dir.join(&root_name);
    let file_count = if root_entry.is_dir {
        download_recursive(
            emitter,
            session,
            channel,
            session_id,
            remote_path,
            &local_path,
        )
        .await?
    } else {
        download_one_file(
            emitter,
            session,
            channel,
            session_id,
            remote_path,
            local_path.clone(),
            Some(root_entry.size),
        )
        .await?;
        1
    };
    Ok(app_logic::dto::DownloadResultDto {
        local_path: local_path.to_string_lossy().into_owned(),
        is_dir: root_entry.is_dir,
        file_count,
    })
}

/// Spec 0054, Teil 2: Herunterladen ohne Dialog, direkt ins
/// Standard-Downloadverzeichnis — Datei oder Ordner (rekursiv).
#[tauri::command]
pub async fn sftp_download_default(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<app_logic::dto::DownloadResultDto> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let downloads_dir = default_downloads_dir()?;
            download_entry_to(
                &TauriEventEmitter(app.clone()),
                &session,
                &channel,
                session_id,
                &remote_path,
                &downloads_dir,
            )
            .await
        },
    )
    .await
}

/// Spec 0054, Teil 2: Ordner-Download an einen per Dialog gewählten
/// Zielort — Gegenstück zu `sftp_download`s Datei-Speichern-Dialog, nur
/// dass ein Ordner keinen Dateinamen zum Speichern hat, sondern ein
/// Zielverzeichnis braucht (`pick_folder` statt `save_file`).
#[tauri::command]
pub async fn sftp_download_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<Option<app_logic::dto::DownloadResultDto>> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            use tauri_plugin_dialog::DialogExt;

            let (tx, rx) = tokio::sync::oneshot::channel();
            app.dialog()
                .file()
                .set_title("Zielordner wählen")
                .pick_folder(move |path| {
                    let _ = tx.send(path);
                });
            let Some(local_dir) = rx.await.ok().flatten() else {
                return Ok(None); // Abbrechen ist kein Fehler.
            };
            let local_dir = local_dir.into_path()?;

            download_entry_to(
                &TauriEventEmitter(app.clone()),
                &session,
                &channel,
                session_id,
                &remote_path,
                &local_dir,
            )
            .await
            .map(Some)
        },
    )
    .await
}

/// `local_path` ist bereits vom Frontend aufgelöst — entweder über den
/// nativen Öffnen-Dialog (Upload-Button, `@tauri-apps/plugin-dialog`, s.
/// `frontend/src/fileDialog.ts` für das bereits etablierte Muster) oder über
/// einen Drag-and-Drop-Vorgang aus dem Betriebssystem (der Pfad kommt dort
/// direkt vom OS-Drop-Ereignis) — beides sind explizite Nutzeraktionen im
/// Sinne von Spec 0020, Abschnitt 5 ("nie ohne expliziten Dialog"), auch
/// wenn der Dialog beim Drag-and-Drop kein Fenster ist, sondern die
/// Drag-Geste selbst.
#[tauri::command]
pub async fn sftp_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    local_path: String,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            upload_impl(
                &TauriEventEmitter(app.clone()),
                &session,
                &channel,
                session_id,
                &local_path,
                &remote_path,
            )
            .await
        },
    )
    .await
}

/// Spec 0086, T8: `emitter` statt eines `AppHandle` — s.
/// [`download_one_file`].
async fn upload_impl(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    local_path: &str,
    remote_path: &str,
) -> CommandResult<()> {
    let file_name = file_name_of(remote_path);

    let local_path_for_stat = local_path.to_string();
    let total_bytes = tokio::task::spawn_blocking(move || {
        std::fs::metadata(&local_path_for_stat)
            .map(|m| m.len())
            .ok()
    })
    .await
    .unwrap_or(None);

    let transfer_id = Uuid::new_v4();
    emit_sftp_transfer_started(
        emitter,
        session_id,
        transfer_id,
        SftpTransferKind::Upload,
        file_name,
        total_bytes,
    );

    let local_path_for_read = local_path.to_string();
    let result: CommandResult<()> = async {
        let bytes = tokio::task::spawn_blocking(move || std::fs::read(local_path_for_read))
            .await
            .map_err(|e| format!("Hintergrund-Task für Upload fehlgeschlagen: {e}"))??;
        let mut guard = lock_browser_sftp(session, channel).await;
        let elevated_user = guard.elevated_user();
        let mut sftp = guard.sftp().await?;
        let written = sftp.write_file(remote_path, &bytes).await;
        audit_elevated_change(
            elevated_user.as_deref(),
            "upload",
            remote_path,
            written.is_ok(),
        );
        written?;
        Ok(())
    }
    .await;

    emit_sftp_transfer_finished(
        emitter,
        session_id,
        transfer_id,
        // Spec 0086, A2.1 — s. `download_one_file`.
        result
            .as_ref()
            .err()
            .map(|e| channel.transfer_error_message(e)),
    );
    result
}

/// Sammelt rekursiv **alle** Verzeichnispfade unter `root` (inklusive
/// `root` selbst) — geteilte Traversierung für `sftp_delete_preview` und
/// den eigentlichen rekursiven Löschvorgang in `sftp_delete` unten. Liefert
/// zusätzlich die Anzahl der gefundenen Dateien, damit ein Aufrufer nicht
/// zweimal denselben Baum ablaufen muss.
///
/// Die zurückgegebenen Verzeichnispfade stehen in **Entdeckungsreihenfolge**
/// (ein Verzeichnis erscheint immer erst NACHDEM sein Elternverzeichnis
/// verarbeitet wurde) — das reicht, um sie für ein bottom-up-Löschen später
/// einfach umzudrehen (`.rev()`), unabhängig davon, ob hier DFS oder BFS
/// traversiert wird (s. `sftp_delete`).
async fn walk_dirs_and_count_files(
    sftp: &mut dyn SftpSession,
    root: &str,
) -> Result<(Vec<String>, u64), SshError> {
    let mut dirs = vec![root.to_string()];
    let mut queue = vec![root.to_string()];
    let mut file_count = 0u64;
    while let Some(dir) = queue.pop() {
        let entries = sftp.list_dir(&dir).await?;
        for entry in entries {
            if entry.is_dir {
                queue.push(entry.path.clone());
                dirs.push(entry.path);
            } else {
                file_count += 1;
            }
        }
    }
    Ok((dirs, file_count))
}

/// Spec 0054, Teil 3: Vorschau vor dem eigentlichen Löschen, analog zum
/// zweistufigen `delete_server` — bei einem Ordner zeigt das Frontend damit
/// "X Dateien, Y Ordner werden gelöscht" statt einer inhaltslosen
/// Ja/Nein-Frage. Für eine Datei ist das Ergebnis trivial (1 Datei, 0
/// Ordner), das Frontend ruft diesen Befehl trotzdem einheitlich für
/// beide Fälle auf.
#[tauri::command]
pub async fn sftp_delete_preview(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<app_logic::dto::DeletePreviewDto> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { delete_preview_impl(&session, &channel, &path).await },
    )
    .await
}

async fn delete_preview_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<app_logic::dto::DeletePreviewDto> {
    use app_logic::dto::DeletePreviewDto;

    let mut guard = lock_browser_sftp(session, channel).await;
    let mut sftp = guard.sftp().await?;

    // `lstat` statt `stat` — dieselbe Symlink-Begründung wie in
    // `delete_recursive` (dieselbe Vorschau soll die Zahlen zeigen, die
    // der anschließende `sftp_delete`-Aufruf tatsächlich löscht).
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        return Ok(DeletePreviewDto {
            file_count: 1,
            dir_count: 0,
        });
    }
    let (dirs, file_count) = walk_dirs_and_count_files(&mut sftp, path).await?;
    Ok(DeletePreviewDto {
        file_count,
        dir_count: dirs.len() as u64,
    })
}

/// Löschen einer Datei ODER eines Ordners (Spec 0054, Teil 3 hebt die
/// bisherige Datei-Beschränkung auf, s. Moduldoc-Kommentar oben) — die
/// Bestätigungsrückfrage selbst läuft im Frontend (Spec 0020, Abschnitt 5),
/// dieser Befehl führt sie nur noch aus.
///
/// Ordner werden bottom-up gelöscht: erst alle Dateien im gesamten Baum
/// (Reihenfolge egal), dann alle Verzeichnisse in umgekehrter
/// Entdeckungsreihenfolge (tiefste zuerst) — SFTP `RMDIR` verlangt ein
/// leeres Verzeichnis, ein Verzeichnis kann also erst entfernt werden,
/// nachdem alles darunter bereits weg ist.
#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { delete_impl(&session, &channel, &path).await },
    )
    .await
}

async fn delete_impl(session: &Session, channel: &BrowserChannel, path: &str) -> CommandResult<()> {
    let mut guard = lock_browser_sftp(session, channel).await;
    // Spec 0085, A1.4: **vor** den Operationen abfragen — nach einem Widerruf
    // mitten in der Rekursion fehlte die Audit-Zeile sonst ganz, obwohl schon
    // Einträge mit erhöhten Rechten gelöscht wurden.
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    let result = delete_recursive(&mut sftp, path).await;
    audit_elevated_change(elevated_user.as_deref(), "delete", path, result.is_ok());
    result?;
    Ok(())
}

/// Eigentliche Rekursions-Logik hinter `sftp_delete` — von der
/// Tauri-Befehls-Signatur (`State<AppState>`, `SessionId`) losgelöst, damit
/// sie sich direkt gegen ein `SftpSession`-Testdouble prüfen lässt (s.
/// `sftp_mutation_tests` unten, gegen den echten lokalen Pseudo-Server via
/// `ssh_transport::LocalFileSession`).
async fn delete_recursive(sftp: &mut dyn SftpSession, path: &str) -> Result<(), SshError> {
    // `lstat` statt `stat` (Spec-Reviewer-Fund, Spec 0054, Review des
    // Gesamtpakets, ERHÖHTE Priorität): `stat` folgt Symlinks — ein
    // Eintrag, der selbst ein Symlink auf ein Verzeichnis ist, würde damit
    // als "ist ein Ordner" erkannt und der Baum DAHINTER (potenziell weit
    // außerhalb des eigentlich sichtbaren Verzeichnisses, z. B.
    // `/var/www/current -> /etc`) rekursiv gelöscht. `lstat` meldet für
    // einen Symlink dessen eigenen Typ, nie den des Ziels — der Symlink
    // selbst wird dann korrekt nur entfernt, nie in ihn hinein rekursiert.
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        sftp.remove(path).await?;
        return Ok(());
    }

    let (dirs, _file_count) = walk_dirs_and_count_files(sftp, path).await?;
    // Alle Dateien im Baum entfernen — dafür noch einmal denselben Baum
    // ablaufen statt die Pfade aus `walk_dirs_and_count_files` zu sammeln:
    // deren Rückgabe zählt Dateien nur, trägt ihre Pfade aber bewusst nicht
    // mit (für die reine Vorschau in `sftp_delete_preview` unnötiger
    // Speicher-/Allokations-Ballast bei großen Bäumen). Der zweite Durchlauf
    // liest dieselben, kleinen Verzeichnislisten erneut — für den ohnehin
    // seltenen "Ordner löschen"-Fall keine spürbare Mehrkosten.
    for dir in &dirs {
        let entries = sftp.list_dir(dir).await?;
        for entry in entries {
            if !entry.is_dir {
                sftp.remove(&entry.path).await?;
            }
        }
    }
    for dir in dirs.into_iter().rev() {
        sftp.remove_dir(&dir).await?;
    }
    Ok(())
}

/// Spec 0054, Teil 3: existiert ein Zielpfad bereits? Grundlage für die
/// Kollisionsprüfung bei "Umbenennen"/"Verschieben" (beide laufen über
/// dieselbe `sftp_rename` unten — SFTP `RENAME` versteht keinen Unterschied
/// zwischen "im selben Ordner umbenennen" und "in einen anderen Ordner
/// verschieben") und bei "Hochladen" (Überschreib-Erkennung vor der
/// Diff-Vorschau). `stat()` ist hier bewusst der einzige Signalweg — kein
/// gesonderter `exists()`-Trait-Befehl, das SFTP-Protokoll kennt ohnehin
/// keine schnellere Existenzprüfung als `STAT`.
#[tauri::command]
pub async fn sftp_exists(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<bool> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { exists_impl(&session, &channel, &path).await },
    )
    .await
}

async fn exists_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<bool> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let mut sftp = guard.sftp().await?;
    // `is_ok()`: „gibt es den Pfad?" ist genau die Frage, ein fehlender Pfad
    // also kein Fehler. Ein **Widerruf** verschwindet dadurch nicht still —
    // er steht im Abbruch-Vermerk des Befehls und macht das Ergebnis beim
    // Abschluss zu `ELEVATED_CHANNEL_INACTIVE` statt zu `Ok(false)` (Spec
    // 0085, A1.2/T6c).
    Ok(sftp.stat(path).await.is_ok())
}

/// Spec 0054, Teil 4: einzelnen Eintrag abfragen — Grundlage für die
/// Konflikt-Prüfung beim "Lokal öffnen"-Upload ("hat sich die Remote-Datei
/// seit dem Download geändert?", s. `local.ts`/`useLocalEditSession`s
/// Vergleich von `RemoteEntryDto.modified` vor Download gegen einen
/// frischen `sftp_stat`-Aufruf vor dem Hochladen). Bislang gab es dafür nur
/// `sftp_list` (ganzes Verzeichnis) und `sftp_exists` (nur `bool`) — ein
/// generischer Einzelabfrage-Befehl fehlte.
#[tauri::command]
pub async fn sftp_stat(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<RemoteEntryDto> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let mut guard = lock_browser_sftp(&session, &channel).await;
            let mut sftp = guard.sftp().await?;
            let entry = sftp.stat(&path).await?;
            Ok(RemoteEntryDto::from(&entry))
        },
    )
    .await
}

/// Spec 0054, Teil 3: chmod. `recursive` gilt nur für Ordner (bei einer
/// Datei ignoriert der Aufrufer das Frontend-seitig ohnehin, s. dortiger
/// Dialog) — läuft denselben Verzeichnisbaum wie `sftp_delete` ab und setzt
/// dieselben Rechte auf **jeden** gefundenen Eintrag (Dateien UND
/// Verzeichnisse selbst), nicht nur auf Blätter.
#[tauri::command]
pub async fn sftp_chmod(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    mode: u32,
    recursive: bool,
    elevated_user: Option<String>,
) -> CommandResult<u64> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            chmod_impl(&session, &channel, &path, mode, recursive).await
        },
    )
    .await
}

async fn chmod_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    mode: u32,
    recursive: bool,
) -> CommandResult<u64> {
    let mut guard = lock_browser_sftp(session, channel).await;
    // Spec 0085, A1.4 — s. `delete_impl`.
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    let result = chmod_recursive(&mut sftp, path, mode, recursive).await;
    audit_elevated_change(elevated_user.as_deref(), "chmod", path, result.is_ok());
    Ok(result?)
}

/// Eigentliche Rekursions-Logik hinter `sftp_chmod` — s. `delete_recursive`s
/// Doc-Kommentar zum selben Testbarkeits-Muster. Liefert die Anzahl der
/// geänderten Einträge (Spec 0067, Teil B: Sammelmeldung).
async fn chmod_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    mode: u32,
    recursive: bool,
) -> Result<u64, SshError> {
    if !recursive {
        sftp.set_permissions(path, mode).await?;
        return Ok(1);
    }
    // `lstat` statt `stat` — dieselbe Symlink-Begründung wie in
    // `delete_recursive`: ein Symlink auf ein Verzeichnis darf ein
    // rekursives chmod nicht in dessen Ziel hinein eskalieren lassen.
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        sftp.set_permissions(path, mode).await?;
        return Ok(1);
    }

    // Erst den ganzen Baum LESEND ablaufen (mit den unveränderten
    // Original-Rechten), ALLE Pfade sammeln, und die Rechte erst danach in
    // einem zweiten Durchlauf setzen. Ohne diese Trennung würde ein bereits
    // umgesetztes Verzeichnis — z. B. `mode` ohne Owner-Execute-Bit — die
    // eigene weitere Traversierung blockieren (ein Verzeichnis ohne `x` für
    // den eigenen Owner lässt sich unter Unix selbst vom Owner-Prozess
    // nicht mehr auflisten), sobald es als Nächstes an der Reihe wäre.
    let mut all_paths = vec![path.to_string()];
    let mut queue = vec![path.to_string()];
    while let Some(dir) = queue.pop() {
        let entries = sftp.list_dir(&dir).await?;
        for entry in entries {
            all_paths.push(entry.path.clone());
            if entry.is_dir {
                queue.push(entry.path);
            }
        }
    }
    // Rückwärts (tiefste zuerst, Wurzel zuletzt) — dieselbe Begründung wie
    // oben: sobald die Wurzel selbst ihr Execute-Bit verliert, lässt sich
    // kein Pfad *unter* ihr mehr auflösen, auch nicht nur für ein weiteres
    // `set_permissions` (Pfadauflösung braucht `x` auf jedem Vorfahren).
    let count = all_paths.len() as u64;
    for entry_path in all_paths.into_iter().rev() {
        sftp.set_permissions(&entry_path, mode).await?;
    }
    Ok(count)
}

/// Spec 0054, Teil 3: "Umbenennen" UND "Verschieben" laufen über denselben
/// Befehl — SFTP `RENAME` unterscheidet nicht zwischen beidem, `to` kann im
/// selben Verzeichnis (Umbenennen) oder einem anderen (Verschieben)
/// liegen. Die Kollisionsprüfung (Zielname existiert schon) läuft im
/// Frontend **vor** diesem Aufruf über `sftp_exists` — dieser Befehl führt
/// nur noch aus, analog zu `sftp_delete`s Bestätigung.
#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    from: String,
    to: String,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let mut guard = lock_browser_sftp(&session, &channel).await;
            let elevated_user = guard.elevated_user();
            let mut sftp = guard.sftp().await?;
            let result = sftp.rename(&from, &to).await;
            audit_elevated_change(
                elevated_user.as_deref(),
                "rename",
                &format!("{from} -> {to}"),
                result.is_ok(),
            );
            result?;
            Ok(())
        },
    )
    .await
}

#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let mut guard = lock_browser_sftp(&session, &channel).await;
            let elevated_user = guard.elevated_user();
            let mut sftp = guard.sftp().await?;
            let result = sftp.create_dir(&path).await;
            audit_elevated_change(elevated_user.as_deref(), "mkdir", &path, result.is_ok());
            result?;
            Ok(())
        },
    )
    .await
}

/// Obergrenze für "Dateiinhalt kopieren" (Spec 0054, Teil 2) UND für die
/// Upload-Diff-Vorschau (Teil 3, `read_local_text_preview` unten). Bewusst
/// **kein** gemeinsamer Code-Pfad und keine gemeinsame Konstante mit
/// `orchestration::ReadRemoteFile`s 256-KB-Cap (Spec 0020, Abschnitt
/// 4.1) — das liefe für einen manuellen Klick über KI-Infrastruktur
/// (Redaction, Filter-Mapping), genau die Vermischung, die Spec 0054s
/// Sicherheitsmodell ausschließt ("manuelle Aktionen laufen NIE durch
/// KI-/Filter-Code, auch nicht nur durch eine Hilfsfunktion davon"). Der
/// gleiche Zahlenwert ist reiner Zufall gleich guter Praxis, keine
/// geteilte Definition.
const MAX_TEXT_PREVIEW_BYTES: u64 = 256 * 1024;

/// Spec 0086, A1.1: die Ablehnungs-Meldung für „Dateiinhalt kopieren" —
/// **wörtlich dieselbe** vor und nach dem Lesen. An einer Stelle gebildet
/// statt an zwei formatiert: der Nutzer soll nicht daran ablesen können, an
/// welcher der beiden Prüfungen es lag (und zwei Formatierungen wären
/// auseinandergelaufen).
fn too_large_for_clipboard() -> CommandError {
    CommandError::from(format!(
        "Datei ist größer als {} KB — zu groß zum Kopieren in die Zwischenablage",
        MAX_TEXT_PREVIEW_BYTES / 1024
    ))
}

/// Spec 0054, Teil 2: "Dateiinhalt kopieren" — liest eine Remote-Datei als
/// Text für die Zwischenablage (der eigentliche `writeText`-Aufruf passiert
/// im Frontend, s. `navigator.clipboard` dort). Kein Filter-Engine-/KI-Gate
/// (Sicherheitsmodell, Spec 0054): eine direkte, unkritische Nutzeraktion,
/// wie jeder andere `sftp_*`-Befehl in diesem Abschnitt.
///
/// Größenprüfung vor dem eigentlichen Lesen (per `stat`), damit eine sehr
/// große Datei nicht erst vollständig übertragen wird, bevor sie doch
/// abgelehnt wird — ein fehlgeschlagenes `stat` blockiert den Lesevorgang
/// selbst nicht (analog zu `sftp_download`s Größen-Vorablauf oben), die
/// eigentliche `read_file`-Fehlermeldung ist dann aussagekräftig genug.
#[tauri::command]
pub async fn sftp_read_text(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
) -> CommandResult<String> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move { read_text_impl(&session, &channel, &path).await },
    )
    .await
}

async fn read_text_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<String> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let mut sftp = guard.sftp().await?;

    // Ein fehlgeschlagenes `stat` blockiert das Lesen nicht (s.
    // Doc-Kommentar). Ein Widerruf **zwischen** `stat` und `read_file` lässt
    // die Datei ungelesen: die zweite Operation prüft ihn selbst (Spec 0085,
    // A1.1/T6b).
    if let Ok(entry) = sftp.stat(path).await {
        if entry.size > MAX_TEXT_PREVIEW_BYTES {
            return Err(too_large_for_clipboard());
        }
    }

    let bytes = sftp.read_file(path).await?;
    // Spec 0086, A1.1: Die `stat`-Prüfung oben ist eine Vorabprüfung, keine
    // Zusage. Die Kanal-Sperre wird je Operation genommen (Spec 0085, A1.3;
    // ADR 0078 §1), zwischen `stat` und `read_file` kann also ein anderer
    // Befehl laufen und die Datei wachsen — und ein gescheitertes `stat`
    // lässt sie bewusst ganz ungeprüft (s. Doc-Kommentar oben). Maßgeblich
    // ist deshalb diese Prüfung am tatsächlich gelesenen Inhalt: nur sie
    // kann die Grenze halten, die der Nutzer zugesagt bekommt.
    if bytes.len() as u64 > MAX_TEXT_PREVIEW_BYTES {
        return Err(too_large_for_clipboard());
    }
    String::from_utf8(bytes)
        .map_err(|_| CommandError::from("Datei ist keine Textdatei (kein gültiges UTF-8)"))
}

/// Spec 0054, Teil 3: die "neue" (lokale) Seite der Upload-Überschreib-Diff-
/// Vorschau — Gegenstück zu `sftp_read_text` für die "alte" (Remote-)Seite,
/// nur **graceful** statt fehlschlagend: eine zu große oder nicht-Text-Datei
/// liefert `text: None` (das Frontend zeigt dann einen Größenvergleich-
/// Hinweis statt eines Diffs, analog zu `BinaryFileChangeHint` bei
/// KI-Dateischreibvorgängen, Spec 0020 Abschnitt 4.2), statt den ganzen
/// Upload-Bestätigungsdialog mit einem Fehler abzubrechen — anders als bei
/// "Dateiinhalt kopieren" ist eine Binärdatei hier ein erwarteter,
/// alltäglicher Fall (Uploads sind keine Textdateien), kein Ausnahmefall.
///
/// `local_path` ist wie bei `sftp_upload` bereits vom Frontend aufgelöst
/// (nativer Öffnen-Dialog oder OS-Drag-and-Drop) — derselbe Vertrauens-
/// Grenzfall, dieselbe Begründung wie dort.
#[tauri::command]
pub async fn read_local_text_preview(
    local_path: String,
) -> CommandResult<app_logic::dto::LocalFilePreviewDto> {
    use app_logic::dto::LocalFilePreviewDto;

    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&local_path))
        .await
        .map_err(|e| format!("Hintergrund-Task für Datei-Vorschau fehlgeschlagen: {e}"))??;
    let size = bytes.len() as u64;
    if size > MAX_TEXT_PREVIEW_BYTES {
        return Ok(LocalFilePreviewDto { text: None, size });
    }
    Ok(LocalFilePreviewDto {
        text: String::from_utf8(bytes).ok(),
        size,
    })
}

// --- Spec 0054, Teil 4: "Lokal öffnen -> bearbeiten -> Upload anbieten" ----
//
// Download in ein **kontrolliertes** Temp-Verzeichnis (Spec-Wortlaut: "nicht
// irgendwo — ein definiertes Temp-Verzeichnis der App"), das eigentliche
// "mit lokalem Programm öffnen" läuft über `@tauri-apps/plugin-opener`s
// bereits registrierte, produktionsreife `openPath`-Funktion direkt im
// Frontend (kein eigener Befehl nötig — s. `capabilities/default.json`s
// neu ergänztes `opener:allow-open-path`). Die lokale Änderungserkennung
// (Datei-Watcher) läuft als Polling auf `local_file_mtime` im Frontend
// statt über einen nativen Dateisystem-Watcher (z. B. `notify`-Crate): für
// eine einzelne, während einer aktiven Bearbeitung beobachtete Datei ist
// ein Poll-Intervall im Sekundenbereich unauffällig genug, um dafür keine
// neue, plattformübergreifend nicht triviale native Abhängigkeit
// einzuführen — s. ADR zu dieser Spec.

/// Basisordner für alle Editier-Temp-Dateien EINER Session — eigener
/// Unterordner pro `session_id`, damit `disconnect()` (unten) beim
/// Trennen der Verbindung gezielt genau diese und keine fremden
/// Editier-Sessions aufräumen kann ("Temp aufräumen bei Session-Ende
/// spätestens", Spec 0054 Teil 4, Punkt 6).
pub(super) fn edit_session_dir(session_id: SessionId) -> CommandResult<std::path::PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or("Kein Cache-Verzeichnis gefunden")
        .map_err(CommandError::from)?;
    Ok(base
        .cache_dir()
        .join("smart-ssh")
        .join("edit-sessions")
        .join(session_id.to_string()))
}

/// Spec 0086, A1.2: Obergrenze für „Lokal öffnen". Bewusst eine **eigene**
/// Konstante ohne gemeinsamen Code mit [`MAX_TEXT_PREVIEW_BYTES`] oder dem
/// KI-Lesepfad (A1.4) — die Begründung am Doc-Kommentar von
/// `MAX_TEXT_PREVIEW_BYTES` gilt hier genauso, und inhaltlich sind es zwei
/// verschiedene Fragen: was sinnvoll in die Zwischenablage passt, und was
/// sinnvoll in einen lokalen Editor geladen wird.
const MAX_EDIT_OPEN_BYTES: u64 = 50 * 1024 * 1024;

/// Spec 0086, A1.2: der Wortlaut der Ablehnung, wörtlich aus der Spec.
/// Fester Text statt aus [`MAX_EDIT_OPEN_BYTES`] formatiert, weil der Wert
/// in Tests als Parameter kleiner gesetzt wird (s.
/// [`open_for_editing_impl`]) — ein formatierter Text würde dort eine
/// Grenze nennen, die es im Produktivbetrieb nicht gibt.
const TOO_LARGE_FOR_EDITING: &str =
    "Datei ist größer als 50 MB — zu groß zum lokalen Öffnen. Bitte stattdessen herunterladen.";

/// Spec 0054, Teil 4, Punkt 1: Download in das kontrollierte
/// Editier-Temp-Verzeichnis dieser Session. Ein erneutes Öffnen derselben
/// Remote-Datei überschreibt die lokale Kopie einfach mit dem aktuellen
/// Remote-Inhalt (keine zweite, veraltete Kopie unter neuem Namen) — wer
/// eine bereits laufende Bearbeitung fortsetzen will, nutzt die
/// weiterhin geöffnete Anwendung, nicht einen erneuten "Lokal öffnen"-Klick.
///
/// Spec 0086, A1.2: Dateien über [`MAX_EDIT_OPEN_BYTES`] werden abgelehnt —
/// vor dem Lesen, wenn `stat` die Größe schon zeigt, sonst nach dem Lesen.
#[tauri::command]
pub async fn sftp_open_for_editing(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<EditSessionDto> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            open_for_editing_impl(
                &session,
                &channel,
                session_id,
                &remote_path,
                MAX_EDIT_OPEN_BYTES,
            )
            .await
        },
    )
    .await
}

/// `max_bytes` ist ein Parameter statt direkt [`MAX_EDIT_OPEN_BYTES`]
/// (Spec 0086, T5): die Prüfung **nach** dem Lesen ließe sich sonst nur mit
/// einem echten 50-MB-Puffer je Testfall abdecken. Der Produktivpfad
/// (`sftp_open_for_editing` oben) gibt immer die echte Konstante mit und ist
/// der einzige **produktive** Aufrufer; die übrigen Aufrufstellen liegen in
/// `revocation_tests.rs` (ebenfalls mit der echten Konstante) und
/// `size_limit_tests.rs` (mit kleinen Werten) — s. ADR 0080.
async fn open_for_editing_impl(
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    remote_path: &str,
    max_bytes: u64,
) -> CommandResult<EditSessionDto> {
    let file_name = file_name_of(remote_path);
    // Zip-Slip-Schutz (s. `safe_local_segment`-Doc-Kommentar) — auch hier
    // landet ein transitiv server-kontrollierter Name als lokales
    // Pfadsegment.
    safe_local_segment(&file_name)?;

    let (bytes, remote_modified) = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        // Zwei Operationen: ein Widerruf dazwischen lässt die Datei
        // ungelesen (Spec 0085, A1.1/T6b).
        let entry = sftp.stat(remote_path).await?;
        // Spec 0086, A1.2: vor dem Lesen, wenn `stat` die Größe schon zeigt —
        // dann wird eine zu große Datei nicht erst komplett übertragen.
        if entry.size > max_bytes {
            return Err(CommandError::from(TOO_LARGE_FOR_EDITING));
        }
        let bytes = sftp.read_file(remote_path).await?;
        (bytes, entry.modified.map(|dt| dt.to_rfc3339()))
    };

    // Spec 0086, A1.2/A1.3: und nach dem Lesen, denn die `stat`-Größe oben
    // ist keine Zusage — die Kanal-Sperre wird je Operation genommen (Spec
    // 0085, A1.3; ADR 0078 §1), die Datei kann zwischen `stat` und
    // `read_file` gewachsen sein. Die Prüfung steht **vor** jedem
    // Schreibzugriff ins Temp-Verzeichnis: bei einer Ablehnung entsteht dort
    // weder Datei noch Verzeichnis, und eine aus einem früheren „Lokal
    // öffnen" schon liegende Kopie bleibt unverändert (A1.3).
    if bytes.len() as u64 > max_bytes {
        return Err(CommandError::from(TOO_LARGE_FOR_EDITING));
    }

    let dir = edit_session_dir(session_id)?;
    let local_path = dir.join(&file_name);
    let local_path_for_write = local_path.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        // Spec-Reviewer-Härtungshinweis (Spec 0054, Review des
        // Gesamtpakets): der Inhalt ist Remote-Serverinhalt (potenziell
        // Passwörter/Keys in einer `.conf`-Datei) — ohne restriktive Unix-
        // Rechte wäre er auf einem Mehrbenutzer-System für andere lokale
        // Nutzer lesbar (Standard-Umask liegt typischerweise bei
        // 0755/0644). `0700`/`0600` schränken auf den eigenen Owner ein.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        // Spec 0067, A5: Datei gleich mit 0600 anlegen statt erst mit den
        // Standardrechten zu schreiben und danach einzuschränken — im
        // erhöhten Modus können das Root-Dateien sein. `set_permissions`
        // bleibt für eine schon vorhandene Datei (`mode` wirkt nur beim
        // Neuanlegen).
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&local_path_for_write)?;
            std::io::Write::write_all(&mut file, &bytes)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &local_path_for_write,
                std::fs::Permissions::from_mode(0o600),
            )?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Hintergrund-Task für lokalen Download fehlgeschlagen: {e}"))??;

    Ok(EditSessionDto {
        local_path: local_path.to_string_lossy().into_owned(),
        remote_modified,
    })
}

/// Spec 0054, Teil 4, Punkt 3/4: Polling-Grundlage für die lokale
/// Änderungserkennung (s. Moduldoc-Kommentar oben zur Polling- statt
/// Watcher-Entscheidung). `Ok(None)` sowohl bei einer nicht (mehr)
/// existierenden Datei als auch bei einem sonstigen Lesefehler — für den
/// Aufrufer (reines "hat sich etwas geändert?"-Polling) ist "kein
/// verlässlicher Zeitstempel verfügbar" in beiden Fällen dieselbe
/// Situation, ein technischer Fehlerdialog dafür wäre für einen
/// Hintergrund-Poll unangemessen aufdringlich.
#[tauri::command]
pub async fn local_file_mtime(local_path: String) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        std::fs::metadata(&local_path)
            .and_then(|m| m.modified())
            .ok()
    })
    .await
    .ok()
    .flatten()
    .map(chrono::DateTime::<chrono::Utc>::from)
    .map(|dt| dt.to_rfc3339())
}

/// Spec 0054, Teil 4, Punkt 6: "Watcher stoppt, wenn der Nutzer den Flow
/// beendet; Temp-Datei aufräumen." Best-effort — eine bereits vom Nutzer
/// oder dem externen Programm gelöschte Datei ist kein Fehlerfall.
#[tauri::command]
pub async fn close_edit_session(session_id: SessionId, local_path: String) -> CommandResult<()> {
    // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): ohne
    // `session_id`-Parameter hätte dieser Befehl JEDEN vom Frontend
    // übergebenen lokalen Pfad gelöscht — bei einem sauberen Frontend
    // passiert das nie, aber als Verteidigung in der Tiefe (dieselbe
    // Webview rendert auch KI-generierten Chat-Inhalt) kostet die
    // Einschränkung auf den eigenen Editier-Temp-Ordner dieser Session
    // nichts an Funktionalität — `close_edit_session` wird ohnehin nie mit
    // einem anderen Pfad aufgerufen.
    let dir = edit_session_dir(session_id)?;
    if !std::path::Path::new(&local_path).starts_with(&dir) {
        return Err(CommandError::from(
            "Pfad liegt außerhalb des Editier-Temp-Ordners dieser Session",
        ));
    }
    match tokio::fs::remove_file(&local_path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CommandError::from(e.to_string())),
    }
}

#[cfg(test)]
mod sftp_mutation_tests {
    use std::os::unix::fs::PermissionsExt;

    use ssh_transport::LocalFileSession;

    use super::*;

    #[tokio::test]
    async fn test_delete_recursive_removes_nested_files_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("sub/b.txt"), b"b").unwrap();
        let mut sftp = LocalFileSession::new();

        delete_recursive(&mut sftp, root.to_str().unwrap())
            .await
            .expect("delete_recursive() sollte gelingen");

        assert!(!root.exists());
    }

    #[tokio::test]
    async fn test_delete_recursive_on_a_plain_file_just_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("solo.txt");
        std::fs::write(&file, b"x").unwrap();
        let mut sftp = LocalFileSession::new();

        delete_recursive(&mut sftp, file.to_str().unwrap())
            .await
            .unwrap();

        assert!(!file.exists());
    }

    /// Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets, ERHÖHTE
    /// Priorität): "Löschen" auf einen Symlink, der auf ein Verzeichnis
    /// AUSSERHALB des eigentlich gemeinten Baums zeigt, darf niemals in
    /// dieses Ziel hinein rekursieren — nur der Symlink selbst wird
    /// entfernt. Verifiziert gegen den un-gefixten Stand: mit `stat` statt
    /// `lstat` als Root-Prüfung (der Zustand vor diesem Fix) schlägt dieser
    /// Test fehl, weil `outside/victim.txt` dann mitgelöscht würde.
    #[tokio::test]
    async fn test_delete_recursive_does_not_follow_a_symlink_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("victim.txt"), b"do not delete me").unwrap();
        let link = dir.path().join("link-to-outside");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let mut sftp = LocalFileSession::new();

        delete_recursive(&mut sftp, link.to_str().unwrap())
            .await
            .expect("delete_recursive() sollte gelingen (löscht nur den Symlink)");

        assert!(!link.exists(), "der Symlink selbst sollte entfernt sein");
        assert!(
            outside.join("victim.txt").exists(),
            "das Symlink-Ziel außerhalb des Baums darf unangetastet bleiben"
        );
    }

    /// Gegenstück für rekursives chmod — dieselbe Symlink-Begründung.
    #[tokio::test]
    async fn test_chmod_recursive_does_not_follow_a_symlink_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, b"x").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
        let link = dir.path().join("link-to-outside");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let mut sftp = LocalFileSession::new();

        chmod_recursive(&mut sftp, link.to_str().unwrap(), 0o700, true)
            .await
            .expect("chmod_recursive() sollte gelingen (setzt nur den Symlink selbst)");

        let victim_entry = sftp.stat(victim.to_str().unwrap()).await.unwrap();
        assert_eq!(
            victim_entry.permissions, 0o644,
            "das Symlink-Ziel außerhalb des Baums darf unangetastet bleiben"
        );
    }

    #[tokio::test]
    async fn test_walk_dirs_and_count_files_counts_the_whole_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("sub/b.txt"), b"b").unwrap();
        std::fs::write(root.join("sub/c.txt"), b"c").unwrap();
        let mut sftp = LocalFileSession::new();

        let (dirs, file_count) = walk_dirs_and_count_files(&mut sftp, root.to_str().unwrap())
            .await
            .unwrap();

        // `dirs` enthält den Wurzelordner selbst plus "sub" — s.
        // `DeletePreviewDto::dir_count`s Doc-Kommentar ("zählt den Ordner
        // selbst mit").
        assert_eq!(dirs.len(), 2);
        assert_eq!(file_count, 3);
    }

    #[tokio::test]
    async fn test_chmod_recursive_without_recursive_flag_only_touches_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(&root).unwrap();
        let child = root.join("child.txt");
        std::fs::write(&child, b"x").unwrap();
        let mut sftp = LocalFileSession::new();

        chmod_recursive(&mut sftp, root.to_str().unwrap(), 0o700, false)
            .await
            .unwrap();

        let root_entry = sftp.stat(root.to_str().unwrap()).await.unwrap();
        let child_entry = sftp.stat(child.to_str().unwrap()).await.unwrap();
        assert_eq!(root_entry.permissions, 0o700);
        assert_ne!(child_entry.permissions, 0o700);
    }

    #[tokio::test]
    async fn test_chmod_recursive_with_recursive_flag_touches_every_entry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let nested = root.join("sub/child.txt");
        std::fs::write(&nested, b"x").unwrap();
        let mut sftp = LocalFileSession::new();

        // 0o700 statt 0o600: das Execute-Bit muss für Verzeichnisse
        // erhalten bleiben, sonst sperrt man sich beim rekursiven chmod
        // selbst aus dem eigenen Baum aus (Unix braucht `x` auf jedem
        // Vorfahren, um einen Pfad darunter überhaupt aufzulösen) — exakt
        // der Bug, den `chmod_recursive`s "erst lesend traversieren, dann
        // von unten nach oben setzen"-Reihenfolge verhindern soll; dieser
        // Test verifiziert das Ergebnis, nicht die Reihenfolge selbst.
        let changed = chmod_recursive(&mut sftp, root.to_str().unwrap(), 0o700, true)
            .await
            .expect("chmod_recursive() sollte gelingen");

        // Spec 0067, Teil B: Anzahl für die Sammelmeldung — tree, sub,
        // sub/child.txt.
        assert_eq!(changed, 3);
        let root_entry = sftp.stat(root.to_str().unwrap()).await.unwrap();
        let sub_entry = sftp.stat(root.join("sub").to_str().unwrap()).await.unwrap();
        let nested_entry = sftp.stat(nested.to_str().unwrap()).await.unwrap();
        assert_eq!(root_entry.permissions, 0o700);
        assert_eq!(sub_entry.permissions, 0o700);
        assert_eq!(nested_entry.permissions, 0o700);
    }
}

/// Spec 0054, Teil 4: die von `AppState`/einer echten SFTP-Session
/// losgelösten Bausteine des "Lokal öffnen"-Flows — `sftp_open_for_editing`
/// selbst bräuchte eine volle `Session` (kein bestehendes Test-Setup dafür,
/// s. Fehlen jeglicher `sftp_*`-Command-Tests auf dieser Ebene schon vor
/// Spec 0054), aber `edit_session_dir`/`local_file_mtime`/
/// `close_edit_session` sind pure bzw. rein-lokale Dateisystem-Funktionen.
#[cfg(test)]
mod edit_session_tests {
    use super::*;

    #[test]
    fn test_edit_session_dir_is_distinct_per_session() {
        let a = edit_session_dir(SessionId::new_v4()).unwrap();
        let b = edit_session_dir(SessionId::new_v4()).unwrap();
        assert_ne!(a, b);
        assert!(a.ends_with(a.file_name().unwrap()));
    }

    #[tokio::test]
    async fn test_local_file_mtime_returns_some_for_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edited.txt");
        std::fs::write(&path, b"x").unwrap();

        let mtime = local_file_mtime(path.to_str().unwrap().to_string()).await;

        assert!(mtime.is_some());
    }

    #[tokio::test]
    async fn test_local_file_mtime_returns_none_for_a_missing_file() {
        let mtime = local_file_mtime("/this/path/does-not-exist-smart-ssh-test".to_string()).await;
        assert!(mtime.is_none());
    }

    #[tokio::test]
    async fn test_close_edit_session_removes_the_file() {
        let session_id = SessionId::new_v4();
        let dir = edit_session_dir(session_id).unwrap();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("edited.txt");
        tokio::fs::write(&path, b"x").await.unwrap();

        close_edit_session(session_id, path.to_str().unwrap().to_string())
            .await
            .expect("close_edit_session() sollte gelingen");

        assert!(!path.exists());
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    /// Spec 0054, Teil 4, Punkt 6: "Watcher stoppt ... Temp-Datei
    /// aufräumen" — ein bereits vom Nutzer oder dem externen Programm
    /// selbst gelöschtes Temp-File ist kein Fehlerfall.
    #[tokio::test]
    async fn test_close_edit_session_on_an_already_missing_file_is_not_an_error() {
        let session_id = SessionId::new_v4();
        let path = edit_session_dir(session_id)
            .unwrap()
            .join("never-existed.txt");

        close_edit_session(session_id, path.to_str().unwrap().to_string())
            .await
            .expect(
                "ein bereits fehlendes Temp-File darf close_edit_session nicht scheitern lassen",
            );
    }

    /// Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): ohne
    /// Session-Eingrenzung hätte `close_edit_session` JEDEN übergebenen
    /// lokalen Pfad gelöscht — Verteidigung in der Tiefe (s. Doc-Kommentar
    /// an `close_edit_session`).
    #[tokio::test]
    async fn test_close_edit_session_rejects_a_path_outside_its_own_session_dir() {
        let session_id = SessionId::new_v4();
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("not-mine.txt");
        std::fs::write(&path, b"x").unwrap();

        let result = close_edit_session(session_id, path.to_str().unwrap().to_string()).await;

        assert!(result.is_err());
        assert!(
            path.exists(),
            "eine fremde Datei darf nicht gelöscht werden"
        );
    }
}
