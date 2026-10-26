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

use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use uuid::Uuid;

use ssh_manager_core::ssh::{RemoteEntry, SftpSession, SshError};

use crate::elevated_sftp::ElevatedSftpRegistry;
use crate::event_emitter::TauriEventEmitter;
use app_logic::dto::{sort_remote_entries, EditSessionDto, RemoteEntryDto};
use app_logic::error::ssh_command_error;
use app_logic::error::CommandError;
use app_logic::error::CommandResult;
use app_logic::error::{
    SFTP_CONTENT_TOO_LARGE, SFTP_COPY_CANCELLED, SFTP_PREVIEW_NOT_TEXT, SFTP_PREVIEW_TOO_LARGE,
};
use app_logic::events::{
    emit_sftp_entries_finished, emit_sftp_entries_progress, emit_sftp_transfer_started,
    EventEmitter, SftpEntriesProgress, SftpTransferKind,
};
use app_logic::local_path_grants::{GrantedLocalPath, LocalPathGrants, LOCAL_PATH_NOT_GRANTED};
use app_logic::session::{Session, SessionManager};
use app_logic::state::{AppState, SessionId};

use super::elevation::{
    audit_elevated_change, file_name_of, lock_browser_sftp, run_browser_command,
    safe_local_segment, BrowserChannel,
};
use crate::file_ops::{OpHandle, OpKind, StopReason};

/// Spec 0085, A1 (T1–T9): Widerruf des erhöhten Modus innerhalb eines schon
/// laufenden Browser-Befehls. Eigene Datei, weil der Aufbau (Test-Double mit
/// Haltepunkt, Audit-Mitschnitt) für sich genommen umfangreich ist.
mod folder_upload;
pub(crate) use folder_upload::*;

mod transfer;
use transfer::{download_file, stopped_error, upload_file, FileRun};

#[cfg(test)]
mod revocation_tests;

/// Spec 0086, A1 (T1–T7): die Größengrenzen von „Dateiinhalt kopieren" und
/// „Lokal öffnen" halten auch bei einer zwischen `stat` und `read_file`
/// gewachsenen Datei. Eigene Datei, weil der Aufbau (Kanal-Double, bei dem
/// `stat` und `read_file` sich widersprechen) für sich genommen umfangreich
/// ist.
#[cfg(test)]
mod size_limit_tests;

/// Issue #89: only local paths the user picked, dropped or that belong to
/// the session's own edit copy are read. Own file because the setup (a
/// session with a mock channel plus grants) is shared by several tests.
#[cfg(test)]
mod local_path_tests;

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

/// Spec 0102: Startpfad des Dateibrowsers dieser Sitzung (`"."` = Home)
/// und ggf. der einmalige Hinweis, dass das konfigurierte Startverzeichnis
/// fehlt. Läuft über den normalen SFTP-Kanal der Sitzung — das
/// Startverzeichnis gilt für den Login-Nutzer, nicht für den erhöhten Modus.
#[tauri::command]
pub async fn sftp_start_directory(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<app_logic::start_directory::StartDirectoryDto> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    Ok(app_logic::start_directory::file_browser_start(&session).await)
}

/// Issue #544: whether the login user of this session is root (`id -u` probe
/// at connect). `None` = unknown. The file browser hides its elevation
/// controls only for `Some(true)`; the backend's elevation checks are unchanged.
#[tauri::command]
pub async fn session_is_root(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<Option<bool>> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    Ok(session.is_root())
}

async fn list_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<Vec<RemoteEntryDto>> {
    let entries = {
        let mut guard = lock_browser_sftp(session, channel).await;
        let mut sftp = guard.sftp().await?;
        let mut entries = sftp.list_dir(path).await?;
        // Issue #653: SFTP v3 servers often send only uid/gid; resolve names
        // once per session from /etc/passwd and /etc/group (SFTP reads only).
        if entries.iter().any(|e| {
            (e.owner.is_none() && e.uid.is_some()) || (e.group.is_none() && e.gid.is_some())
        }) {
            let names = session
                .id_names_or_init(|| app_logic::owner_names::load_id_names(&mut sftp))
                .await;
            app_logic::owner_names::apply_id_names(names, &mut entries);
        }
        entries
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
    // Issue #622: in Stücken, mit Fortschritt und Abbruch — s. `transfer`.
    let transfer_id = Uuid::new_v4();
    let guard = channel.register_op(transfer_id, OpKind::Download, remote_path);
    guard.handle().set_bytes_total(total_bytes);
    let run = FileRun {
        emitter,
        session,
        channel,
        session_id,
        op: guard.handle(),
        folder: false,
    };
    download_file(&run, transfer_id, remote_path, local_path, total_bytes, 0).await?;
    Ok(())
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
    // Issue #679: a `content://` URI (Android) is never a path. The file is
    // downloaded into an app-private staging copy and then written through
    // the URI; progress and cancellation work as for any download.
    let handle = crate::local_file::LocalFileHandle::from_dialog(local_path)?;
    let (target, staging) = match handle.as_path() {
        Some(p) => (p.to_path_buf(), None),
        None => {
            let dir = crate::edit_copies::picked_root()
                .ok_or_else(|| CommandError::from("No cache directory available"))?
                .join(Uuid::new_v4().to_string());
            (dir.join(&file_name), Some(dir))
        }
    };
    if let Some(dir) = &staging {
        std::fs::create_dir_all(dir)?;
    }

    let downloaded = download_one_file(
        &TauriEventEmitter(app.clone()),
        session,
        channel,
        session_id,
        remote_path,
        target.clone(),
        total_bytes,
    )
    .await;
    let copied = match (&staging, &downloaded) {
        (Some(_), Ok(())) => {
            let opener = crate::local_file::platform_opener(app);
            let from = target.clone();
            let h = handle.clone();
            tokio::task::spawn_blocking(move || h.copy_from(opener.as_ref(), &from))
                .await
                .map_err(|e| CommandError::from(e.to_string()))
                .and_then(|r| r.map(|_| ()).map_err(CommandError::from))
        }
        _ => Ok(()),
    };
    if let Some(dir) = &staging {
        if let Err(err) = std::fs::remove_dir_all(dir) {
            tracing::warn!(error = %err, "could not remove the staged download");
        }
    }
    downloaded?;
    copied?;
    Ok(Some(app_logic::dto::DownloadResultDto {
        local_path: handle.display(),
        is_dir: false,
        file_count: 1,
    }))
}

/// Folder picker. Desktop only: `tauri-plugin-dialog` offers no folder
/// picker on Android (Spec 0114); there the UI does not show the actions and
/// a direct call is refused. `None` = the user cancelled.
#[cfg(not(target_os = "android"))]
async fn pick_folder_dialog(
    app: &AppHandle,
    title: &str,
) -> CommandResult<Option<std::path::PathBuf>> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .pick_folder(move |path| {
            let _ = tx.send(path);
        });
    match rx.await.ok().flatten() {
        Some(picked) => Ok(Some(picked.into_path()?)),
        None => Ok(None),
    }
}

#[cfg(target_os = "android")]
async fn pick_folder_dialog(
    _app: &AppHandle,
    _title: &str,
) -> CommandResult<Option<std::path::PathBuf>> {
    Err(CommandError::from(
        "Folder selection is not available on Android",
    ))
}

/// Ermittelt das Standard-Downloadverzeichnis des Betriebssystems (Spec
/// 0054, Teil 2: "Standard-Downloadverzeichnis ODER präziser Pfad per
/// Dialog") — `directories::UserDirs` ist bereits Projektabhängigkeit (s.
/// `logging.rs`).
fn default_downloads_dir() -> CommandResult<std::path::PathBuf> {
    // Android (Spec 0114): kein `HOME`; app-privater Ordner als Ersatz.
    #[cfg(target_os = "android")]
    {
        Ok(persistence_sqlite::android_files_dir().join("downloads"))
    }
    #[cfg(not(target_os = "android"))]
    {
        directories::UserDirs::new()
            .and_then(|dirs| dirs.download_dir().map(|p| p.to_path_buf()))
            .ok_or("Kein Standard-Downloadverzeichnis gefunden")
            .map_err(CommandError::from)
    }
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
    // Issue #622: ein Vorgang für den ganzen Ordner (Bestätigungsdialog,
    // Abbrechen), erst den Baum durchlaufen (Dateizahl und Gesamtgröße
    // bekannt), dann Datei für Datei in Stücken laden.
    let op_guard = channel.register_op(Uuid::new_v4(), OpKind::Download, remote_root);
    let op = op_guard.handle();
    tokio::fs::create_dir_all(local_root).await?;
    let mut files: Vec<(String, std::path::PathBuf, u64)> = Vec::new();
    let mut queue = vec![(remote_root.to_string(), local_root.to_path_buf())];
    while let Some((remote_dir, local_dir)) = queue.pop() {
        if op.is_user_stopped() {
            return Err(stopped_error());
        }
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
                files.push((entry.path, local_entry_path, entry.size));
            }
        }
    }
    op.set_files(0, Some(files.len() as u64));
    op.set_bytes_total(Some(files.iter().map(|f| f.2).sum()));
    let run = FileRun {
        emitter,
        session,
        channel,
        session_id,
        op,
        folder: true,
    };
    let mut file_count = 0u64;
    let mut base_bytes = 0u64;
    for (remote_path, local_path, size) in files {
        if op.is_user_stopped() {
            return Err(stopped_error());
        }
        base_bytes += download_file(
            &run,
            Uuid::new_v4(),
            &remote_path,
            local_path,
            Some(size),
            base_bytes,
        )
        .await?;
        op.add_files_done();
        file_count += 1;
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
/// Zielverzeichnis braucht (`pick_folder` statt `save_file`). Issue #153:
/// the dialog title comes translated from the frontend (pattern:
/// `pick_upload_files`); it is display-only.
#[tauri::command]
pub async fn sftp_download_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    remote_path: String,
    title: String,
    elevated_user: Option<String>,
) -> CommandResult<Option<app_logic::dto::DownloadResultDto>> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let Some(local_dir) = pick_folder_dialog(&app, &title).await? else {
                return Ok(None); // Abbrechen ist kein Fehler.
            };

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

/// Issue #622: cancel button of a transfer row. Stops the transfer (for a
/// folder transfer: the whole folder) at the next chunk; the row shows
/// "cancelled". `false`: no such running transfer (already finished).
#[tauri::command]
pub async fn sftp_cancel_transfer(
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    transfer_id: Uuid,
) -> CommandResult<bool> {
    Ok(elevated.ops().cancel(session_id, transfer_id))
}

/// Issue #89: `local_path` comes from the webview and proves nothing on its
/// own — the webview also renders AI-generated content. It is read only if
/// it lies inside a grant of this session (native open dialog via
/// [`pick_upload_files`], native drop via [`claim_dropped_paths`], or the
/// session's own edit-session directory), see
/// `app_logic::local_path_grants`. Any other path fails before a channel
/// is opened, anything is read or a transfer starts.
// Tauri command arguments are the IPC interface; grouping them into a
// struct would change the frontend call shape.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn sftp_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_path: String,
    remote_path: String,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    let edit_root = crate::edit_copies::instance_root();
    let local_path = authorize_local_path(
        &state.sessions,
        grants.inner(),
        edit_root.as_deref(),
        session_id,
        &local_path,
    )?;
    let staged = local_path.as_path().to_path_buf();
    let result = run_browser_command(
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
    .await;
    // Issue #679: a copy staged from the Android picker is not needed after
    // the upload (success or not); real files are never inside this root.
    if let Some(root) = crate::edit_copies::picked_root() {
        crate::edit_copies::remove_staged_pick(&staged, &root);
    }
    result
}

/// Issue #89: the one place where a frontend-supplied local path becomes a
/// [`GrantedLocalPath`]. The session must still exist (grants die with it,
/// and a leftover edit-session directory of a closed session must not count
/// either), then the path must lie inside a grant of exactly this session.
///
/// `edit_root`: the edit-copy root, see [`edit_session_dir_in`]. Every
/// command passes [`crate::edit_copies::instance_root`].
pub(super) fn authorize_local_path(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> CommandResult<GrantedLocalPath> {
    if sessions.get(session_id).is_none() {
        return Err(CommandError::from(LOCAL_PATH_NOT_GRANTED));
    }
    let edit_dir = edit_session_dir_in(edit_root, session_id).ok();
    grants.check(
        session_id,
        std::path::Path::new(local_path),
        edit_dir.as_deref(),
    )
}

/// Issue #89: the upload button's native open dialog, run in the backend
/// (pattern: `read_credential_file`). The frontend passes only a title;
/// the picked paths are granted to this session and returned. `None` =
/// cancelled. Files only — folders go through `pick_upload_folder` (ADR 0144).
#[tauri::command]
pub async fn pick_upload_files(
    app: AppHandle,
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    title: String,
) -> CommandResult<Option<Vec<String>>> {
    use tauri_plugin_dialog::DialogExt;

    if state.sessions.get(session_id).is_none() {
        return Err(CommandError::from("Session nicht gefunden"));
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(&title)
        .pick_files(move |paths| {
            let _ = tx.send(paths);
        });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None); // Abbrechen ist kein Fehler.
    };
    // Issue #679: `content://` URIs (Android) are copied into an app-private
    // staging directory; the copies are what gets granted and uploaded.
    let staging = crate::edit_copies::picked_root().map(|r| r.join(Uuid::new_v4().to_string()));
    let mut paths = Vec::with_capacity(picked.len());
    for item in picked {
        let handle = crate::local_file::LocalFileHandle::from_dialog(item)?;
        let path = match handle.as_path() {
            Some(p) => p.to_path_buf(),
            None => {
                let dir = staging
                    .as_deref()
                    .ok_or_else(|| CommandError::from("No cache directory available"))?;
                // Same display name twice: keep both copies apart.
                let dir = dir.join(paths.len().to_string());
                let h = handle.clone();
                let o = crate::local_file::platform_opener(&app);
                tokio::task::spawn_blocking(move || h.stage_into(o.as_ref(), &dir))
                    .await
                    .map_err(|e| CommandError::from(e.to_string()))??
            }
        };
        paths.push(path);
    }
    Ok(Some(grant_for_live_session(
        &state.sessions,
        grants.inner(),
        session_id,
        paths,
    )))
}

/// Issue #89: hands the most recent native drop (captured from the window's
/// drag-and-drop event in the backend, see `crate::run`) to this session's
/// file browser and grants its paths. The frontend sends no paths — only
/// which session claims the drop. Empty list = no fresh drop to claim.
#[tauri::command]
pub async fn claim_dropped_paths(
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
) -> CommandResult<Vec<String>> {
    claim_dropped_paths_impl(&state.sessions, grants.inner(), session_id)
}

pub(super) fn claim_dropped_paths_impl(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    session_id: SessionId,
) -> CommandResult<Vec<String>> {
    if sessions.get(session_id).is_none() {
        return Err(CommandError::from("Session nicht gefunden"));
    }
    Ok(grants
        .claim_drop(session_id)
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

/// Issue #571: whether the OS clipboard holds at least one local file or
/// folder reference. Returns no paths — it only decides whether the empty-
/// space menu offers "Paste". An unreadable clipboard counts as "no files".
#[tauri::command]
pub async fn clipboard_has_files(
    clipboard: State<'_, crate::file_clipboard::FileClipboard>,
) -> CommandResult<bool> {
    match clipboard.get_files() {
        Ok(files) => Ok(!files.is_empty()),
        Err(err) => {
            tracing::debug!(error = %err, "clipboard could not be read for the paste check");
            Ok(false)
        }
    }
}

/// Issue #626: writes `text` to the OS clipboard from the backend. The
/// webview refuses `navigator.clipboard.writeText` (WebKit) once the user
/// gesture is gone, e.g. after awaiting file content.
#[tauri::command]
pub async fn clipboard_write_text(
    clipboard: State<'_, crate::file_clipboard::FileClipboard>,
    text: String,
) -> CommandResult<()> {
    clipboard.set_text(&text).map_err(|e| {
        tracing::debug!(error = %e, "clipboard text write failed");
        CommandError::from("The clipboard could not be written")
    })
}

/// Texts of the paste confirmation dialog; the frontend supplies the
/// localized wording, the file list and target are added by the backend.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardPasteDialogTexts {
    pub title: String,
    pub intro: String,
    pub target_label: String,
    pub more_label: String,
    pub confirm_label: String,
    pub cancel_label: String,
}

/// Maximum number of names listed in the confirmation dialog.
const PASTE_DIALOG_MAX_NAMES: usize = 12;

fn paste_dialog_message(
    texts: &ClipboardPasteDialogTexts,
    files: &[std::path::PathBuf],
    target_dir: &str,
) -> String {
    let mut message = format!("{}\n", texts.intro);
    for file in files.iter().take(PASTE_DIALOG_MAX_NAMES) {
        message.push_str(&format!("\n• {}", file.display()));
    }
    if files.len() > PASTE_DIALOG_MAX_NAMES {
        message.push_str(&format!(
            "\n• {}",
            texts.more_label.replace(
                "{count}",
                &(files.len() - PASTE_DIALOG_MAX_NAMES).to_string()
            )
        ));
    }
    message.push_str(&format!("\n\n{}: {}", texts.target_label, target_dir));
    message
}

/// Core of [`claim_clipboard_files`]: nothing is granted unless `confirm`
/// returns `true` (the user clicked the native dialog's confirm button) and
/// the session still exists. `None` = nothing on the clipboard; `Some(empty)`
/// = cancelled.
pub(super) async fn claim_clipboard_files_impl<F, Fut>(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    session_id: SessionId,
    files: Vec<std::path::PathBuf>,
    confirm: F,
) -> CommandResult<Option<Vec<String>>>
where
    F: FnOnce(Vec<std::path::PathBuf>) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    if sessions.get(session_id).is_none() {
        return Err(CommandError::from("Session nicht gefunden"));
    }
    if files.is_empty() {
        return Ok(None);
    }
    if !confirm(files.clone()).await {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(grant_for_live_session(
        sessions, grants, session_id, files,
    )))
}

/// Issue #571: "Paste" of local files from the OS clipboard as an upload.
/// The backend reads the clipboard itself, shows a native confirmation that
/// lists the files and the target directory, and only after the user
/// confirmed grants the paths to this session and returns them (third grant
/// source next to the open dialog and the native drop, ADR 0113/0145). The
/// webview sends no local path; `target_dir` is display text only.
/// `None` = nothing on the clipboard, empty list = cancelled.
#[tauri::command]
pub async fn claim_clipboard_files(
    app: AppHandle,
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    clipboard: State<'_, crate::file_clipboard::FileClipboard>,
    session_id: SessionId,
    target_dir: String,
    texts: ClipboardPasteDialogTexts,
) -> CommandResult<Option<Vec<String>>> {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

    if state.sessions.get(session_id).is_none() {
        return Err(CommandError::from("Session nicht gefunden"));
    }
    let files = clipboard.get_files().map_err(|err| {
        tracing::debug!(error = %err, "clipboard could not be read for paste");
        CommandError::from("The clipboard could not be read")
    })?;
    claim_clipboard_files_impl(
        &state.sessions,
        grants.inner(),
        session_id,
        files,
        |files| async move {
            let message = paste_dialog_message(&texts, &files, &target_dir);
            let (tx, rx) = tokio::sync::oneshot::channel();
            app.dialog()
                .message(message)
                .title(&texts.title)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    texts.confirm_label.clone(),
                    texts.cancel_label.clone(),
                ))
                .show(move |confirmed| {
                    let _ = tx.send(confirmed);
                });
            // A dialog that vanishes without an answer counts as cancel.
            rx.await.unwrap_or(false)
        },
    )
    .await
}

/// Grants `paths` only if the session is still there once the dialog
/// returns — a session closed while the dialog was open gets no grants.
/// A disconnect between this check and the grant leaves an entry for a
/// session id that is never used again; every reader checks that the
/// session exists ([`authorize_local_path`]).
pub(super) fn grant_for_live_session(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    session_id: SessionId,
    paths: Vec<std::path::PathBuf>,
) -> Vec<String> {
    if sessions.get(session_id).is_none() {
        return Vec::new();
    }
    grants
        .grant(session_id, paths)
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Spec 0086, T8: `emitter` statt eines `AppHandle` — s.
/// [`download_one_file`].
async fn upload_impl(
    emitter: &dyn EventEmitter,
    session: &Session,
    channel: &BrowserChannel,
    session_id: SessionId,
    local_path: &GrantedLocalPath,
    remote_path: &str,
) -> CommandResult<()> {
    // Issue #622: in Stücken, mit Fortschritt und Abbruch — s. `transfer`.
    let transfer_id = Uuid::new_v4();
    let guard = channel.register_op(transfer_id, OpKind::Upload, remote_path);
    let run = FileRun {
        emitter,
        session,
        channel,
        session_id,
        op: guard.handle(),
        folder: false,
    };
    upload_file(&run, transfer_id, local_path.as_path(), remote_path, 0).await?;
    Ok(())
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

/// Issue #658: the row of a recursive delete/chmod. A ticker reads the
/// operation's entry counters (monotonic, only ever increasing) and emits
/// throttled progress; the row appears once the recursion found a folder, so
/// a single file or a non-recursive chmod never gets one.
struct EntryRows<'a> {
    sink: Option<(&'a dyn EventEmitter, SessionId)>,
    kind: SftpTransferKind,
    op: OpHandle,
    path: String,
    started: bool,
}

impl<'a> EntryRows<'a> {
    fn new(
        sink: Option<(&'a dyn EventEmitter, SessionId)>,
        kind: SftpTransferKind,
        op: &OpHandle,
        path: &str,
    ) -> Self {
        Self {
            sink,
            kind,
            op: op.clone(),
            path: path.to_string(),
            started: false,
        }
    }

    /// Emits `started` (once) and the current counts, if the row exists.
    fn sync(&mut self) {
        let (Some((emitter, session_id)), Some((done, total))) = (self.sink, self.op.entries())
        else {
            return;
        };
        if !self.started {
            self.started = true;
            emit_sftp_transfer_started(
                emitter,
                session_id,
                self.op.id(),
                self.kind,
                self.path.clone(),
                None,
            );
        }
        emit_sftp_entries_progress(
            emitter,
            session_id,
            self.op.id(),
            SftpEntriesProgress { done, total },
        );
    }

    async fn run<T>(&mut self, work: impl std::future::Future<Output = T>) -> T {
        if self.sink.is_none() {
            return work.await;
        }
        tokio::pin!(work);
        let mut tick = tokio::time::interval(crate::file_ops::PROGRESS_MIN_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                r = &mut work => return r,
                _ = tick.tick() => self.sync(),
            }
        }
    }

    /// Final state: done / cancelled / stopped / failed, with the number of
    /// entries changed. `Ok(false)` = stopped before the end.
    fn finish(&mut self, outcome: Result<bool, String>) {
        self.sync();
        let (Some((emitter, session_id)), true) = (self.sink, self.started) else {
            return;
        };
        let done = self.op.entries().map_or(0, |(d, _)| d);
        let (error, stopped) = match outcome {
            Ok(true) => (None, None),
            Ok(false) => (
                None,
                Some(match self.op.stop_reason() {
                    Some(StopReason::Cancelled) => "cancelled",
                    _ => "stopped",
                }),
            ),
            Err(e) => (Some(e), None),
        };
        emit_sftp_entries_finished(emitter, session_id, self.op.id(), error, stopped, done);
    }
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
    app: AppHandle,
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
            let emitter = TauriEventEmitter(app);
            delete_impl_reporting(Some((&emitter, session_id)), &session, &channel, &path).await
        },
    )
    .await
}

#[cfg(test)]
async fn delete_impl(session: &Session, channel: &BrowserChannel, path: &str) -> CommandResult<()> {
    delete_impl_reporting(None, session, channel, path).await
}

/// Issue #658: a recursive delete of a folder shows a row (events
/// `sftp-transfer-*` with kind `delete`); `progress: None` runs silently.
async fn delete_impl_reporting(
    progress: Option<(&dyn EventEmitter, SessionId)>,
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<()> {
    let mut guard = lock_browser_sftp(session, channel).await;
    // Spec 0085, A1.4: **vor** den Operationen abfragen — nach einem Widerruf
    // mitten in der Rekursion fehlte die Audit-Zeile sonst ganz, obwohl schon
    // Einträge mit erhöhten Rechten gelöscht wurden.
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    // Issue #622: als laufende Operation eingetragen (Bestätigungsdialog).
    let op = channel.register_op(Uuid::new_v4(), OpKind::Delete, path);
    let mut rows = EntryRows::new(progress, SftpTransferKind::Delete, op.handle(), path);
    let result = rows
        .run(delete_recursive(&mut sftp, path, op.handle()))
        .await;
    rows.finish(
        result
            .as_ref()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string()),
    );
    audit_elevated_change(
        elevated_user.as_deref(),
        "delete",
        path,
        matches!(result, Ok(Some(()))),
    );
    match result? {
        Some(()) => Ok(()),
        None => Err(stopped_error()),
    }
}

/// Eigentliche Rekursions-Logik hinter `sftp_delete` — von der
/// Tauri-Befehls-Signatur (`State<AppState>`, `SessionId`) losgelöst, damit
/// sie sich direkt gegen ein `SftpSession`-Testdouble prüfen lässt (s.
/// `sftp_mutation_tests` unten, gegen den echten lokalen Pseudo-Server via
/// `ssh_transport::LocalFileSession`).
///
/// Issue #622: `Ok(None)` = vom Nutzer gestoppt; geprüft wird vor jedem
/// Eintrag, schon Gelöschtes bleibt gelöscht (wie bei einem Widerruf, Spec
/// 0085).
async fn delete_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    op: &OpHandle,
) -> Result<Option<()>, SshError> {
    // `lstat` statt `stat` (Spec-Reviewer-Fund, Spec 0054, Review des
    // Gesamtpakets, ERHÖHTE Priorität): `stat` folgt Symlinks — ein
    // Eintrag, der selbst ein Symlink auf ein Verzeichnis ist, würde damit
    // als "ist ein Ordner" erkannt und der Baum DAHINTER (potenziell weit
    // außerhalb des eigentlich sichtbaren Verzeichnisses, z. B.
    // `/var/www/current -> /etc`) rekursiv gelöscht. `lstat` meldet für
    // einen Symlink dessen eigenen Typ, nie den des Ziels — der Symlink
    // selbst wird dann korrekt nur entfernt, nie in ihn hinein rekursiert.
    if op.is_user_stopped() {
        return Ok(None);
    }
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        sftp.remove(path).await?;
        return Ok(Some(()));
    }

    // Issue #658: from here on the operation counts entries (row + progress).
    op.begin_entries();
    let (dirs, file_count) = walk_dirs_and_count_files(sftp, path).await?;
    op.set_files(0, Some(dirs.len() as u64 + file_count));
    // Alle Dateien im Baum entfernen — dafür noch einmal denselben Baum
    // ablaufen statt die Pfade aus `walk_dirs_and_count_files` zu sammeln:
    // deren Rückgabe zählt Dateien nur, trägt ihre Pfade aber bewusst nicht
    // mit (für die reine Vorschau in `sftp_delete_preview` unnötiger
    // Speicher-/Allokations-Ballast bei großen Bäumen). Der zweite Durchlauf
    // liest dieselben, kleinen Verzeichnislisten erneut — für den ohnehin
    // seltenen "Ordner löschen"-Fall keine spürbare Mehrkosten.
    for dir in &dirs {
        if op.is_user_stopped() {
            return Ok(None);
        }
        let entries = sftp.list_dir(dir).await?;
        for entry in entries {
            if !entry.is_dir {
                if op.is_user_stopped() {
                    return Ok(None);
                }
                sftp.remove(&entry.path).await?;
                op.add_files_done();
            }
        }
    }
    for dir in dirs.into_iter().rev() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        sftp.remove_dir(&dir).await?;
        op.add_files_done();
    }
    Ok(Some(()))
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
#[allow(clippy::too_many_arguments)] // one argument per IPC parameter
pub async fn sftp_chmod(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    mode: u32,
    recursive: bool,
    apply_special_bits: Option<bool>,
    elevated_user: Option<String>,
) -> CommandResult<u64> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let emitter = TauriEventEmitter(app);
            chmod_impl_reporting(
                Some((&emitter, session_id)),
                &session,
                &channel,
                &path,
                mode,
                recursive,
                apply_special_bits.unwrap_or(false),
            )
            .await
        },
    )
    .await
}

#[cfg(test)]
async fn chmod_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    mode: u32,
    recursive: bool,
    apply_special_bits: bool,
) -> CommandResult<u64> {
    chmod_impl_reporting(
        None,
        session,
        channel,
        path,
        mode,
        recursive,
        apply_special_bits,
    )
    .await
}

/// Issue #658: s. `delete_impl_reporting` (kind `chmod`).
async fn chmod_impl_reporting(
    progress: Option<(&dyn EventEmitter, SessionId)>,
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    mode: u32,
    recursive: bool,
    apply_special_bits: bool,
) -> CommandResult<u64> {
    let mut guard = lock_browser_sftp(session, channel).await;
    // Spec 0085, A1.4 — s. `delete_impl`.
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    let op = channel.register_op(Uuid::new_v4(), OpKind::Chmod, path);
    let mut rows = EntryRows::new(progress, SftpTransferKind::Chmod, op.handle(), path);
    let result = rows
        .run(chmod_recursive(
            &mut sftp,
            path,
            mode,
            recursive,
            apply_special_bits,
            op.handle(),
        ))
        .await;
    rows.finish(
        result
            .as_ref()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string()),
    );
    audit_elevated_change(
        elevated_user.as_deref(),
        "chmod",
        path,
        matches!(result, Ok(Some(_))),
    );
    result?.ok_or_else(stopped_error)
}

/// Issue #624: the mode one entry gets during a recursive apply (like
/// `chmod -R` with capital `X`). Files get exactly the chosen rwx bits and
/// never a special bit. Folders get the chosen rwx bits plus execute for
/// every class that has read, so they stay enterable; their special bits
/// stay as they are unless the user changed them (`apply_special_bits`),
/// in which case the chosen ones apply to folders only.
fn recursive_entry_mode(entry: &RemoteEntry, mode: u32, apply_special_bits: bool) -> u32 {
    let rwx = mode & 0o777;
    if !entry.is_dir {
        return rwx;
    }
    let rwx = rwx | ((rwx & 0o444) >> 2);
    let special = if apply_special_bits {
        mode & 0o7000
    } else {
        entry.permissions & 0o7000
    };
    rwx | special
}

/// Eigentliche Rekursions-Logik hinter `sftp_chmod` — s. `delete_recursive`s
/// Doc-Kommentar zum selben Testbarkeits-Muster. Liefert die Anzahl der
/// geänderten Einträge (Spec 0067, Teil B: Sammelmeldung).
async fn chmod_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    mode: u32,
    recursive: bool,
    apply_special_bits: bool,
    op: &OpHandle,
) -> Result<Option<u64>, SshError> {
    if !recursive {
        sftp.set_permissions(path, mode).await?;
        return Ok(Some(1));
    }
    // `lstat` statt `stat` — dieselbe Symlink-Begründung wie in
    // `delete_recursive`: ein Symlink auf ein Verzeichnis darf ein
    // rekursives chmod nicht in dessen Ziel hinein eskalieren lassen.
    let root_entry = sftp.lstat(path).await?;
    if !root_entry.is_dir {
        sftp.set_permissions(path, mode).await?;
        return Ok(Some(1));
    }

    // Erst den ganzen Baum LESEND ablaufen (mit den unveränderten
    // Original-Rechten), ALLE Pfade sammeln, und die Rechte erst danach in
    // einem zweiten Durchlauf setzen. Ohne diese Trennung würde ein bereits
    // umgesetztes Verzeichnis — z. B. `mode` ohne Owner-Execute-Bit — die
    // eigene weitere Traversierung blockieren (ein Verzeichnis ohne `x` für
    // den eigenen Owner lässt sich unter Unix selbst vom Owner-Prozess
    // nicht mehr auflisten), sobald es als Nächstes an der Reihe wäre.
    op.begin_entries();
    let mut all_paths = vec![(
        path.to_string(),
        recursive_entry_mode(&root_entry, mode, apply_special_bits),
    )];
    let mut queue = vec![path.to_string()];
    while let Some(dir) = queue.pop() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        let entries = sftp.list_dir(&dir).await?;
        for entry in entries {
            all_paths.push((
                entry.path.clone(),
                recursive_entry_mode(&entry, mode, apply_special_bits),
            ));
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
    op.set_files(0, Some(count));
    for (entry_path, entry_mode) in all_paths.into_iter().rev() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        sftp.set_permissions(&entry_path, entry_mode).await?;
        op.add_files_done();
    }
    Ok(Some(count))
}

/// Issue #654: change owner and/or group. `owner`/`group` are a name (looked
/// up in the server's `/etc/passwd`/`/etc/group`) or a number; `None` or an
/// empty text leaves that side unchanged. Names are resolved **before**
/// anything is changed: an unknown name fails with nothing touched.
/// `recursive` applies to folders only and never follows symlinks.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // one argument per IPC parameter
pub async fn sftp_chown(
    app: AppHandle,
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    path: String,
    owner: Option<String>,
    group: Option<String>,
    recursive: bool,
    elevated_user: Option<String>,
) -> CommandResult<u64> {
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let emitter = TauriEventEmitter(app);
            chown_impl_reporting(
                Some((&emitter, session_id)),
                &session,
                &channel,
                &path,
                owner.as_deref(),
                group.as_deref(),
                recursive,
            )
            .await
        },
    )
    .await
}

#[cfg(test)]
async fn chown_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    owner: Option<&str>,
    group: Option<&str>,
    recursive: bool,
) -> CommandResult<u64> {
    chown_impl_reporting(None, session, channel, path, owner, group, recursive).await
}

pub(crate) const CHOWN_UNKNOWN_USER: &str = "CHOWN_UNKNOWN_USER";
pub(crate) const CHOWN_UNKNOWN_GROUP: &str = "CHOWN_UNKNOWN_GROUP";
pub(crate) const CHOWN_NOTHING_TO_CHANGE: &str = "CHOWN_NOTHING_TO_CHANGE";

fn non_empty(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

fn is_numeric_id(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
}

/// Resolves the typed owner/group to ids. Numbers need no lookup; a name is
/// looked up on the server through the **same channel** as the change (so an
/// elevated change reads the files with the elevated rights and a revoked
/// channel is noticed).
async fn resolve_chown_ids(
    sftp: &mut dyn SftpSession,
    owner: Option<&str>,
    group: Option<&str>,
) -> CommandResult<(Option<u32>, Option<u32>)> {
    use app_logic::owner_names::{load_id_names, resolve_group, resolve_user, IdLookupError};
    let (owner, group) = (non_empty(owner), non_empty(group));
    if owner.is_none() && group.is_none() {
        return Err(CommandError::with_code(
            "Neither owner nor group was given.",
            CHOWN_NOTHING_TO_CHANGE,
        ));
    }
    let needs_lookup =
        owner.is_some_and(|o| !is_numeric_id(o)) || group.is_some_and(|g| !is_numeric_id(g));
    let names = if needs_lookup {
        load_id_names(sftp).await
    } else {
        app_logic::owner_names::IdNames::default()
    };
    let map_err = |e: IdLookupError| match e {
        IdLookupError::UnknownUser(n) => {
            CommandError::with_code(format!("Unknown user: {n}"), CHOWN_UNKNOWN_USER)
        }
        IdLookupError::UnknownGroup(n) => {
            CommandError::with_code(format!("Unknown group: {n}"), CHOWN_UNKNOWN_GROUP)
        }
    };
    let uid = owner
        .map(|o| resolve_user(&names, o))
        .transpose()
        .map_err(map_err)?;
    let gid = group
        .map(|g| resolve_group(&names, g))
        .transpose()
        .map_err(map_err)?;
    Ok((uid, gid))
}

async fn chown_impl_reporting(
    progress: Option<(&dyn EventEmitter, SessionId)>,
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    owner: Option<&str>,
    group: Option<&str>,
    recursive: bool,
) -> CommandResult<u64> {
    let mut guard = lock_browser_sftp(session, channel).await;
    // Spec 0085, A1.4 — s. `delete_impl`.
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    let (uid, gid) = resolve_chown_ids(&mut sftp, owner, group).await?;
    let op = channel.register_op(Uuid::new_v4(), OpKind::Chown, path);
    let mut rows = EntryRows::new(progress, SftpTransferKind::Chown, op.handle(), path);
    let result = rows
        .run(chown_recursive(
            &mut sftp,
            path,
            uid,
            gid,
            recursive,
            op.handle(),
        ))
        .await;
    rows.finish(
        result
            .as_ref()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string()),
    );
    audit_elevated_change(
        elevated_user.as_deref(),
        "chown",
        path,
        matches!(result, Ok(Some(_))),
    );
    result?.ok_or_else(stopped_error)
}

/// SFTP `SETSTAT` takes uid and gid as a pair: the side that stays gets the
/// entry's current id. Without a known current id nothing is guessed.
fn chown_target(
    entry: &RemoteEntry,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<(u32, u32), SshError> {
    let missing = || {
        SshError::ChannelError(format!(
            "{}: the server did not report the current owner/group",
            entry.path
        ))
    };
    Ok((
        uid.or(entry.uid).ok_or_else(missing)?,
        gid.or(entry.gid).ok_or_else(missing)?,
    ))
}

/// Core of `sftp_chown`; mirrors `chmod_recursive` (read the whole tree
/// first, then change deepest-first). Entries that are symlinks are skipped
/// below the root so a link can never lead the change outside the tree.
/// Returns the number of changed entries.
async fn chown_recursive(
    sftp: &mut dyn SftpSession,
    path: &str,
    uid: Option<u32>,
    gid: Option<u32>,
    recursive: bool,
    op: &OpHandle,
) -> Result<Option<u64>, SshError> {
    if op.is_user_stopped() {
        return Ok(None);
    }
    // `lstat`: same symlink reasoning as `delete_recursive`.
    let root_entry = sftp.lstat(path).await?;
    if !recursive || !root_entry.is_dir {
        let (u, g) = chown_target(&root_entry, uid, gid)?;
        sftp.set_owner(path, u, g).await?;
        return Ok(Some(1));
    }
    op.begin_entries();
    let mut targets = vec![(path.to_string(), chown_target(&root_entry, uid, gid)?)];
    let mut queue = vec![path.to_string()];
    while let Some(dir) = queue.pop() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        for entry in sftp.list_dir(&dir).await? {
            if entry.is_symlink {
                continue;
            }
            targets.push((entry.path.clone(), chown_target(&entry, uid, gid)?));
            if entry.is_dir {
                queue.push(entry.path);
            }
        }
    }
    let count = targets.len() as u64;
    op.set_files(0, Some(count));
    for (entry_path, (u, g)) in targets.into_iter().rev() {
        if op.is_user_stopped() {
            return Ok(None);
        }
        sftp.set_owner(&entry_path, u, g).await?;
        op.add_files_done();
    }
    Ok(Some(count))
}

/// Spec 0054, Teil 3: "Umbenennen" UND "Verschieben" laufen über denselben
/// Befehl — SFTP `RENAME` unterscheidet nicht zwischen beidem, `to` kann im
/// selben Verzeichnis (Umbenennen) oder einem anderen (Verschieben)
/// liegen. Die Kollisionsprüfung (Zielname existiert schon) läuft im
/// Frontend **vor** diesem Aufruf über `sftp_exists` — dieser Befehl führt
/// nur noch aus, analog zu `sftp_delete`s Bestätigung.
/// Issue #625: Umbenennen/Verschieben auf einen vorhandenen **Dateinamen**,
/// nachdem der Nutzer „Überschreiben" bestätigt hat. SFTP `RENAME` ersetzt nie.
///
/// - Ziel ist ein Ordner: Fehler, nichts wird angefasst.
/// - Server mit `posix-rename@openssh.com`: ein atomarer Austausch.
/// - Sonst: Ziel entfernen, dann `rename`. Scheitert das `rename`, nennt der
///   Fehler, dass das Ziel schon entfernt ist und die Quelle unverändert bleibt.
///
/// Ein Fehler beim `stat` des Ziels gilt als „Ziel fehlt": dann ersetzt
/// nichts etwas, und das gewöhnliche `rename` entscheidet (es überschreibt
/// nie, und ein Widerruf der Erhöhung schlägt dort genauso an).
async fn rename_replacing(
    sftp: &mut dyn SftpSession,
    from: &str,
    to: &str,
) -> Result<(), SshError> {
    if from == to {
        return Ok(());
    }
    match sftp.stat(to).await {
        Ok(entry) if entry.is_dir => {
            return Err(SshError::ChannelError(format!(
                "Das Ziel '{to}' ist ein Ordner und kann nicht ersetzt werden."
            )));
        }
        Ok(_) => {}
        Err(_) => return sftp.rename(from, to).await,
    }
    if sftp.posix_rename(from, to).await? {
        return Ok(());
    }
    sftp.remove(to).await?;
    sftp.rename(from, to).await.map_err(|e| {
        SshError::ChannelError(format!(
            "Das Ziel '{to}' wurde bereits entfernt, aber das Umbenennen von '{from}' ist \
             fehlgeschlagen; die Quelle ist unverändert: {e}"
        ))
    })
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    from: String,
    to: String,
    overwrite: Option<bool>,
    elevated_user: Option<String>,
) -> CommandResult<()> {
    let overwrite = overwrite.unwrap_or(false);
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            let mut guard = lock_browser_sftp(&session, &channel).await;
            let elevated_user = guard.elevated_user();
            let mut sftp = guard.sftp().await?;
            let result = if overwrite {
                rename_replacing(&mut sftp, &from, &to).await
            } else {
                sftp.rename(&from, &to).await
            };
            audit_elevated_change(
                elevated_user.as_deref(),
                if overwrite {
                    "rename-overwrite"
                } else {
                    "rename"
                },
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
        |session, channel| async move { mkdir_impl(&session, &channel, &path).await },
    )
    .await
}

async fn mkdir_impl(session: &Session, channel: &BrowserChannel, path: &str) -> CommandResult<()> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    // Issue #570: ein vorhandener Name (Datei oder Ordner) wird mit
    // demselben Fehler gemeldet wie bei „Neue Datei". Scheitert die
    // Vorabprüfung selbst, entscheidet `create_dir` allein.
    let result = if sftp.lstat(path).await.is_ok() {
        Err(SshError::SftpAlreadyExists(path.to_string()))
    } else {
        sftp.create_dir(path).await
    };
    audit_elevated_change(elevated_user.as_deref(), "mkdir", path, result.is_ok());
    // Mit Code (z. B. `SSH_SFTP_ALREADY_EXISTS`), damit die Oberfläche die
    // Fehlerart ohne Textvergleich erkennt.
    result.map_err(|err| ssh_command_error(&err))
}

async fn create_file_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
) -> CommandResult<()> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let elevated_user = guard.elevated_user();
    let mut sftp = guard.sftp().await?;
    let result = sftp.create_file_exclusive(path).await;
    audit_elevated_change(
        elevated_user.as_deref(),
        "create_file",
        path,
        result.is_ok(),
    );
    // Mit Code (z. B. `SSH_SFTP_ALREADY_EXISTS`), damit die Oberfläche die
    // Fehlerart ohne Textvergleich erkennt.
    result.map_err(|err| ssh_command_error(&err))
}

/// Issue #570: „Neue Datei" im Dateibrowser — legt eine leere Datei im
/// aktuellen Verzeichnis an, schmal und benannt wie `sftp_mkdir` (Invariante
/// 14: kein Filter-/KI-Pfad). Ein vorhandener Name wird nie überschrieben
/// (CREATE|EXCL, `SSH_SFTP_ALREADY_EXISTS`); fehlende Rechte werden einmal
/// versucht und gemeldet. Im erhöhten Modus läuft der Aufruf über den
/// erhöhten Kanal und wird wie `sftp_mkdir` protokolliert.
#[tauri::command]
pub async fn sftp_create_file(
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
        |session, channel| async move { create_file_impl(&session, &channel, &path).await },
    )
    .await
}

/// Obergrenze für die Remote-Seite der Upload-Diff-Vorschau (`sftp_read_text`;
/// „Dateiinhalt kopieren" hat seit Issue #357 eine eigene, größere Grenze,
/// [`MAX_COPY_CONTENT_BYTES`]) UND für die lokale Seite (Teil 3, `read_local_text_preview` unten). Bewusst
/// **kein** gemeinsamer Code-Pfad und keine gemeinsame Konstante mit
/// `orchestration::ReadRemoteFile`s 256-KB-Cap (Spec 0020, Abschnitt
/// 4.1) — das liefe für einen manuellen Klick über KI-Infrastruktur
/// (Redaction, Filter-Mapping), genau die Vermischung, die Spec 0054s
/// Sicherheitsmodell ausschließt ("manuelle Aktionen laufen NIE durch
/// KI-/Filter-Code, auch nicht nur durch eine Hilfsfunktion davon"). Der
/// gleiche Zahlenwert ist reiner Zufall gleich guter Praxis, keine
/// geteilte Definition.
const MAX_TEXT_PREVIEW_BYTES: u64 = 256 * 1024;

/// Spec 0086, A1.6: die Ablehnungs-Meldung der Vorschau (`sftp_read_text`) —
/// **wörtlich dieselbe** vor und nach dem Lesen. An einer Stelle gebildet
/// statt an zwei formatiert: der Nutzer soll nicht daran ablesen können, an
/// welcher der beiden Prüfungen es lag (und zwei Formatierungen wären
/// auseinandergelaufen).
fn too_large_for_clipboard() -> CommandError {
    CommandError::with_code(
        format!(
            "Datei ist größer als {} KB — zu groß zum Kopieren in die Zwischenablage",
            MAX_TEXT_PREVIEW_BYTES / 1024
        ),
        SFTP_PREVIEW_TOO_LARGE,
    )
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
    String::from_utf8(bytes).map_err(|_| {
        CommandError::with_code(
            "Datei ist keine Textdatei (kein gültiges UTF-8)",
            SFTP_PREVIEW_NOT_TEXT,
        )
    })
}

/// Issue #357, Spec 0086 A1.1: Obergrenze für „Dateiinhalt kopieren"
/// (50 × 1024 × 1024 Bytes, fest, normaler und erhöhter Kanal). Eine Datei
/// von genau dieser Größe ist erlaubt. Unabhängig von
/// [`MAX_TEXT_PREVIEW_BYTES`] (Vorschau) und dem KI-Lesepfad.
const MAX_COPY_CONTENT_BYTES: u64 = 50 * 1024 * 1024;

/// Fortschritt wird in Schritten dieser Größe gemeldet, nicht je Lesestück.
const COPY_PROGRESS_STEP_BYTES: u64 = 1024 * 1024;

fn content_too_large_for_clipboard() -> CommandError {
    CommandError::with_code(
        "The file is larger than 50 MB and cannot be copied to the clipboard.",
        SFTP_CONTENT_TOO_LARGE,
    )
}

fn copy_cancelled() -> CommandError {
    CommandError::with_code("Copying was cancelled.", SFTP_COPY_CANCELLED)
}

/// Fortschritt von „Dateiinhalt kopieren": `done` Bytes gelesen, `total`
/// die per `stat` gemeldete Größe (`None`, wenn `stat` scheiterte).
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyContentProgress {
    pub done: u64,
    pub total: Option<u64>,
}

/// Laufende „Dateiinhalt kopieren"-Aufträge nach Anfrage-ID, damit
/// `sftp_cancel_copy_content` den Lesevorgang beenden kann.
#[derive(Default)]
pub struct CopyContentJobs(
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Notify>>>,
);

impl CopyContentJobs {
    fn register(&self, request_id: &str) -> std::sync::Arc<tokio::sync::Notify> {
        let notify = std::sync::Arc::new(tokio::sync::Notify::new());
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(request_id.to_string(), notify.clone());
        notify
    }

    fn remove(&self, request_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(request_id);
    }

    fn cancel(&self, request_id: &str) {
        if let Some(notify) = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(request_id)
        {
            notify.notify_one();
        }
    }
}

/// Issue #357: „Dateiinhalt kopieren" — liest eine Remote-Textdatei bis
/// 50 MB für die Zwischenablage (der `writeText`-Aufruf passiert im
/// Frontend). Anders als [`sftp_read_text`] (Vorschau des Upload-Diffs,
/// 256 KB) liest dieser Befehl **begrenzt**: nach Grenze + 1 Bytes ist
/// Schluss, auch wenn `stat` scheitert; der Speicher hält nie mehr. Der
/// Fortschritt geht über `on_progress`, `sftp_cancel_copy_content` bricht
/// ab. Kein Filter-Engine-/KI-Gate (Spec 0054, Sicherheitsmodell).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sftp_copy_content(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    jobs: State<'_, CopyContentJobs>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
    request_id: String,
    on_progress: Channel<CopyContentProgress>,
) -> CommandResult<String> {
    let cancel = jobs.register(&request_id);
    let result = run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| {
            let cancel = cancel.clone();
            async move {
                let mut send = |p: CopyContentProgress| {
                    let _ = on_progress.send(p);
                };
                copy_content_impl(
                    &session,
                    &channel,
                    &path,
                    MAX_COPY_CONTENT_BYTES,
                    &cancel,
                    &mut send,
                )
                .await
            }
        },
    )
    .await;
    jobs.remove(&request_id);
    result
}

/// Bricht einen laufenden `sftp_copy_content` ab (unbekannte ID: nichts).
#[tauri::command]
pub async fn sftp_cancel_copy_content(
    jobs: State<'_, CopyContentJobs>,
    request_id: String,
) -> CommandResult<()> {
    jobs.cancel(&request_id);
    Ok(())
}

async fn copy_content_impl(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    limit: u64,
    cancel: &tokio::sync::Notify,
    report: &mut (dyn FnMut(CopyContentProgress) + Send),
) -> CommandResult<String> {
    // Der Abbruch gilt auch, solange der Kanal noch von einem anderen Befehl
    // gehalten wird; das Verwerfen des Futures gibt die Sperre frei.
    tokio::select! {
        result = copy_content_read(session, channel, path, limit, report) => result,
        _ = cancel.notified() => Err(copy_cancelled()),
    }
}

async fn copy_content_read_bytes(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    limit: u64,
    report: &mut (dyn FnMut(CopyContentProgress) + Send),
) -> CommandResult<Vec<u8>> {
    let mut guard = lock_browser_sftp(session, channel).await;
    let mut sftp = guard.sftp().await?;

    // Ein fehlgeschlagenes `stat` blockiert das Lesen nicht; die Grenze hält
    // dann das begrenzte Lesen unten.
    let mut total = None;
    if let Ok(entry) = sftp.stat(path).await {
        if entry.size > limit {
            return Err(content_too_large_for_clipboard());
        }
        total = Some(entry.size);
    }

    let mut last_reported = 0u64;
    let mut on_chunk = |done: u64| {
        if last_reported == 0 || done >= last_reported + COPY_PROGRESS_STEP_BYTES {
            last_reported = done;
            report(CopyContentProgress { done, total });
        }
    };
    // Grenze + 1: ein Byte mehr als erlaubt beweist die Überschreitung, ohne
    // den Rest der Datei zu lesen.
    let bytes = sftp
        .read_file_capped(path, limit.saturating_add(1), &mut on_chunk)
        .await?;
    if bytes.len() as u64 > limit {
        return Err(content_too_large_for_clipboard());
    }
    Ok(bytes)
}

async fn copy_content_read(
    session: &Session,
    channel: &BrowserChannel,
    path: &str,
    limit: u64,
    report: &mut (dyn FnMut(CopyContentProgress) + Send),
) -> CommandResult<String> {
    let bytes = copy_content_read_bytes(session, channel, path, limit, report).await?;
    String::from_utf8(bytes).map_err(|_| {
        CommandError::with_code(
            "Datei ist keine Textdatei (kein gültiges UTF-8)",
            SFTP_PREVIEW_NOT_TEXT,
        )
    })
}

/// Issue #358: "Copy file" — reads a remote file (any type) up to 50 MB with
/// the same bounded read, progress and cancel as [`sftp_copy_content`]
/// (cancel via `sftp_cancel_copy_content`), stages it in the app-private
/// clipboard folder under its original name and puts it on the system
/// clipboard as a file reference. Nothing is opened or executed. A cancelled
/// or failed copy leaves no staged file and does not touch the clipboard; the
/// previous staged file is removed only after the new one is on the clipboard.
/// Returns the file name. No filter-engine/AI gate (Spec 0054 security model).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sftp_copy_file(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    jobs: State<'_, CopyContentJobs>,
    clipboard: State<'_, crate::file_clipboard::FileClipboard>,
    session_id: SessionId,
    path: String,
    elevated_user: Option<String>,
    request_id: String,
    on_progress: Channel<CopyContentProgress>,
) -> CommandResult<()> {
    let file_name = file_name_of(&path);
    safe_local_segment(&file_name)?;
    let cancel = jobs.register(&request_id);
    let result = run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| {
            let cancel = cancel.clone();
            async move {
                let mut send = |p: CopyContentProgress| {
                    let _ = on_progress.send(p);
                };
                tokio::select! {
                    result = copy_content_read_bytes(
                        &session, &channel, &path, MAX_COPY_CONTENT_BYTES, &mut send,
                    ) => result,
                    _ = cancel.notified() => Err(copy_cancelled()),
                }
            }
        },
    )
    .await;
    jobs.remove(&request_id);
    let bytes = result?;

    let root = crate::edit_copies::clipboard_root()
        .ok_or_else(|| CommandError::from("No cache directory available for the clipboard"))?;
    let staged = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || {
            crate::file_clipboard::stage_file(&root, &file_name, &bytes)
        })
        .await
        .map_err(|e| format!("Background task for Copy file failed: {e}"))??
    };
    if let Err(err) = clipboard.set_file(&staged) {
        crate::file_clipboard::discard(&staged);
        return Err(CommandError::with_code(
            format!("The file could not be placed on the clipboard: {err}"),
            app_logic::error::CLIPBOARD_FILE_FAILED,
        ));
    }
    crate::file_clipboard::retire_others(&root, &staged);
    Ok(())
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
/// Issue #89: same rule as [`sftp_upload`] — `local_path` must lie inside a
/// grant of this session, otherwise nothing is read.
#[tauri::command]
pub async fn read_local_text_preview(
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_path: String,
) -> CommandResult<app_logic::dto::LocalFilePreviewDto> {
    read_local_text_preview_impl(
        &state.sessions,
        grants.inner(),
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
    )
    .await
}

pub(super) async fn read_local_text_preview_impl(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> CommandResult<app_logic::dto::LocalFilePreviewDto> {
    use app_logic::dto::LocalFilePreviewDto;

    let local_path = authorize_local_path(sessions, grants, edit_root, session_id, local_path)?;
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(local_path.as_path()))
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
///
/// Issue #93: liegt im Editier-Kopien-Ordner **dieser Instanz** (je
/// Datenverzeichnis, s. [`crate::edit_copies`]), den der Start von
/// Überbleibseln eines abgestürzten Prozesses befreit.
pub(crate) fn edit_session_dir(session_id: SessionId) -> CommandResult<std::path::PathBuf> {
    edit_session_dir_in(crate::edit_copies::instance_root().as_deref(), session_id)
}

/// [`edit_session_dir`] below an explicitly given edit-copy root (`None` =
/// the OS reports no cache directory, which stays an error here).
///
/// Issue #134: the root is a **parameter** of every `*_impl` function that
/// derives an edit-session directory (`authorize_local_path`,
/// `open_for_editing_impl`, `close_edit_session_impl`, …), and each Tauri
/// command passes [`crate::edit_copies::instance_root`] — the same value
/// the old code read inside. Tests pass a `tempfile::tempdir()` of their
/// own instead, so they never touch the real user cache, and the guard
/// removes it even when a test panics. Chosen over a settable test
/// override: a parameter has no process-global (or thread-local) state one
/// test could change under another running in parallel, and nothing
/// test-specific is compiled into the production build — the commands
/// simply have no other value to pass.
pub(super) fn edit_session_dir_in(
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
) -> CommandResult<std::path::PathBuf> {
    let root = edit_root
        .ok_or("Kein Cache-Verzeichnis gefunden")
        .map_err(CommandError::from)?;
    Ok(root.join(session_id.to_string()))
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
    let edit_root = crate::edit_copies::instance_root();
    run_browser_command(
        &state,
        elevated.inner(),
        session_id,
        elevated_user,
        |session, channel| async move {
            open_for_editing_impl(
                &session,
                &channel,
                edit_root.as_deref(),
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
///
/// `edit_root`: s. [`edit_session_dir_in`] (Issue #134).
async fn open_for_editing_impl(
    session: &Session,
    channel: &BrowserChannel,
    edit_root: Option<&std::path::Path>,
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

    let dir = edit_session_dir_in(edit_root, session_id)?;
    let local_path = dir.join(&file_name);
    let local_path_for_write = local_path.clone();
    // Spec-Reviewer-Härtungshinweis (Spec 0054) / Spec 0067, A5: `0700`/
    // `0600` auf Unix, s. `write_edit_copy`.
    tokio::task::spawn_blocking(move || {
        crate::edit_copies::write_edit_copy(&dir, &local_path_for_write, &bytes)
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
///
/// Issue #89: same rule as [`sftp_upload`]. A path outside every grant of
/// this session also yields `None` — for a background poll a refusal is the
/// same "no reliable timestamp", and answering `None` for both a missing
/// and a refused path keeps the command from telling the webview which
/// arbitrary paths exist. The refusal itself is logged by the grant check.
#[tauri::command]
pub async fn local_file_mtime(
    state: State<'_, AppState>,
    grants: State<'_, LocalPathGrants>,
    session_id: SessionId,
    local_path: String,
) -> CommandResult<Option<String>> {
    // Always `Ok` — `CommandResult` only because Tauri requires it for an
    // async command with borrowed `State` arguments.
    Ok(local_file_mtime_impl(
        &state.sessions,
        grants.inner(),
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
    )
    .await)
}

pub(super) async fn local_file_mtime_impl(
    sessions: &SessionManager,
    grants: &LocalPathGrants,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> Option<String> {
    let local_path =
        authorize_local_path(sessions, grants, edit_root, session_id, local_path).ok()?;
    tokio::task::spawn_blocking(move || {
        std::fs::metadata(local_path.as_path())
            .and_then(|m| m.modified())
            .ok()
    })
    .await
    .ok()
    .flatten()
    .map(chrono::DateTime::<chrono::Utc>::from)
    .map(|dt| dt.to_rfc3339())
}

/// Issue #679: upper bound for the in-app editor of an edit copy (Android has
/// no external editor to hand the copy to). Larger files are refused.
const MAX_IN_APP_EDIT_BYTES: u64 = 2 * 1024 * 1024;

/// Resolves `local_path` to a file inside this session's own edit-copy
/// directory (nothing else — not any other granted path), canonicalised.
fn in_app_edit_copy(
    sessions: &SessionManager,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> CommandResult<std::path::PathBuf> {
    if sessions.get(session_id).is_none() {
        return Err(CommandError::from(LOCAL_PATH_NOT_GRANTED));
    }
    let dir = edit_session_dir_in(edit_root, session_id)?
        .canonicalize()
        .map_err(|_| CommandError::from(LOCAL_PATH_NOT_GRANTED))?;
    let canonical = std::path::Path::new(local_path)
        .canonicalize()
        .map_err(|_| CommandError::from(LOCAL_PATH_NOT_GRANTED))?;
    if canonical.starts_with(&dir) && canonical.is_file() {
        Ok(canonical)
    } else {
        Err(CommandError::from(LOCAL_PATH_NOT_GRANTED))
    }
}

pub(super) fn read_edit_copy_text_impl(
    sessions: &SessionManager,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
    max: u64,
) -> CommandResult<String> {
    use std::io::Read;
    let path = in_app_edit_copy(sessions, edit_root, session_id, local_path)?;
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(max + 1)
        .read_to_string(&mut text)
        .map_err(|_| CommandError::from("The file is not valid UTF-8 text"))?;
    if text.len() as u64 > max {
        return Err(CommandError::from("The file is too large to edit here"));
    }
    Ok(text)
}

pub(super) fn write_edit_copy_text_impl(
    sessions: &SessionManager,
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
    content: &str,
    max: u64,
) -> CommandResult<()> {
    if content.len() as u64 > max {
        return Err(CommandError::from("The text is too large to save here"));
    }
    let path = in_app_edit_copy(sessions, edit_root, session_id, local_path)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Issue #679: text of an edit copy for the in-app editor (Android). Only a
/// file inside this session's edit-copy directory is accepted.
#[tauri::command]
pub async fn read_edit_copy_text(
    state: State<'_, AppState>,
    session_id: SessionId,
    local_path: String,
) -> CommandResult<String> {
    read_edit_copy_text_impl(
        &state.sessions,
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
        MAX_IN_APP_EDIT_BYTES,
    )
}

/// Issue #679: saves the in-app editor's text into the edit copy. The
/// existing change detection then offers the upload as for any edit.
#[tauri::command]
pub async fn write_edit_copy_text(
    state: State<'_, AppState>,
    session_id: SessionId,
    local_path: String,
    content: String,
) -> CommandResult<()> {
    write_edit_copy_text_impl(
        &state.sessions,
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
        &content,
        MAX_IN_APP_EDIT_BYTES,
    )
}

/// Spec 0054, Teil 4, Punkt 6: "Watcher stoppt, wenn der Nutzer den Flow
/// beendet; Temp-Datei aufräumen." Best-effort — eine bereits vom Nutzer
/// oder dem externen Programm gelöschte Datei ist kein Fehlerfall.
#[tauri::command]
pub async fn close_edit_session(session_id: SessionId, local_path: String) -> CommandResult<()> {
    close_edit_session_impl(
        crate::edit_copies::instance_root().as_deref(),
        session_id,
        &local_path,
    )
    .await
}

/// `edit_root`: s. [`edit_session_dir_in`] (Issue #134).
async fn close_edit_session_impl(
    edit_root: Option<&std::path::Path>,
    session_id: SessionId,
    local_path: &str,
) -> CommandResult<()> {
    // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): ohne
    // `session_id`-Parameter hätte dieser Befehl JEDEN vom Frontend
    // übergebenen lokalen Pfad gelöscht — bei einem sauberen Frontend
    // passiert das nie, aber als Verteidigung in der Tiefe (dieselbe
    // Webview rendert auch KI-generierten Chat-Inhalt) kostet die
    // Einschränkung auf den eigenen Editier-Temp-Ordner dieser Session
    // nichts an Funktionalität — `close_edit_session` wird ohnehin nie mit
    // einem anderen Pfad aufgerufen.
    let dir = edit_session_dir_in(edit_root, session_id)?;
    if !std::path::Path::new(local_path).starts_with(&dir) {
        return Err(CommandError::from(
            "Pfad liegt außerhalb des Editier-Temp-Ordners dieser Session",
        ));
    }
    match tokio::fs::remove_file(local_path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CommandError::from(e.to_string())),
    }
}

#[cfg(test)]
mod rename_overwrite_tests {
    use ssh_manager_core::ssh::mock::MockSftpSession;

    use super::*;

    #[tokio::test]
    async fn test_overwrite_uses_posix_rename_when_offered() {
        let mut sftp = MockSftpSession::new()
            .with_file("/a", "new")
            .with_file("/b", "old")
            .with_posix_rename();
        rename_replacing(&mut sftp, "/a", "/b").await.unwrap();
        assert_eq!(sftp.file_content("/b").unwrap(), b"new");
        assert!(sftp.file_content("/a").is_none());
        let calls = sftp.calls();
        assert!(calls.contains(&"posix_rename /a -> /b".to_string()));
        assert!(!calls.iter().any(|c| c.starts_with("remove ")));
    }

    #[tokio::test]
    async fn test_overwrite_without_posix_rename_removes_then_renames() {
        let mut sftp = MockSftpSession::new()
            .with_file("/a", "new")
            .with_file("/b", "old");
        rename_replacing(&mut sftp, "/a", "/b").await.unwrap();
        assert_eq!(sftp.file_content("/b").unwrap(), b"new");
        let calls = sftp.calls();
        let rm = calls.iter().position(|c| c == "remove /b").unwrap();
        let mv = calls.iter().position(|c| c == "rename /a -> /b").unwrap();
        assert!(rm < mv);
    }

    #[tokio::test]
    async fn test_plain_rename_onto_existing_file_does_not_overwrite() {
        // Ohne `overwrite` ruft der Befehl nur `rename`; der Mock überschreibt
        // zwar (er kennt keinen Konflikt), entscheidend ist hier: kein
        // `remove`, kein `posix_rename` auf diesem Pfad.
        let mut sftp = MockSftpSession::new()
            .with_file("/a", "new")
            .with_posix_rename();
        sftp.rename("/a", "/b").await.unwrap();
        let calls = sftp.calls();
        assert_eq!(calls, vec!["rename /a -> /b".to_string()]);
    }

    #[tokio::test]
    async fn test_overwrite_with_failing_rename_names_the_situation() {
        let mut sftp = MockSftpSession::new()
            .with_file("/a", "new")
            .with_file("/b", "old")
            .with_rename_failure("/a");
        let err = rename_replacing(&mut sftp, "/a", "/b").await.unwrap_err();
        let SshError::ChannelError(text) = err else {
            panic!("unexpected error");
        };
        assert!(text.contains("bereits entfernt"), "{text}");
        assert!(text.contains("Quelle ist unverändert"), "{text}");
        assert!(sftp.file_content("/b").is_none());
        assert_eq!(sftp.file_content("/a").unwrap(), b"new");
    }

    #[tokio::test]
    async fn test_overwrite_onto_folder_is_refused_untouched() {
        let mut sftp = MockSftpSession::new()
            .with_file("/a", "new")
            .with_dir("/d")
            .with_posix_rename();
        let err = rename_replacing(&mut sftp, "/a", "/d").await.unwrap_err();
        assert!(matches!(err, SshError::ChannelError(t) if t.contains("Ordner")));
        assert!(!sftp
            .calls()
            .iter()
            .any(|c| c.starts_with("remove") || c.contains("rename")));
    }
}

#[cfg(test)]
mod sftp_mutation_tests {
    use ssh_transport::LocalFileSession;

    use super::*;

    // --- Issue #658: rows with progress and cancel for recursive delete/chmod ---

    use std::sync::Arc;

    use app_logic::events::TestEmitter;
    use async_trait::async_trait;
    use ssh_manager_core::ssh::{ChunkControl, ChunkSource, TransferEnd};

    use crate::file_ops::FileOps;

    /// Wraps a real local session and cancels the operation (as the row's
    /// cancel button does) after `after` changing calls.
    struct CancelAfter {
        inner: LocalFileSession,
        ops: Arc<FileOps>,
        session_id: SessionId,
        op_id: Uuid,
        after: usize,
        calls: usize,
    }

    impl CancelAfter {
        fn tick(&mut self) {
            self.calls += 1;
            if self.calls == self.after {
                assert!(self.ops.cancel(self.session_id, self.op_id));
            }
        }
    }

    #[async_trait]
    impl SftpSession for CancelAfter {
        async fn list_dir(&mut self, p: &str) -> Result<Vec<RemoteEntry>, SshError> {
            self.inner.list_dir(p).await
        }
        async fn read_file(&mut self, p: &str) -> Result<Vec<u8>, SshError> {
            self.inner.read_file(p).await
        }
        async fn read_file_capped(
            &mut self,
            p: &str,
            max: u64,
            progress: &mut (dyn FnMut(u64) + Send),
        ) -> Result<Vec<u8>, SshError> {
            self.inner.read_file_capped(p, max, progress).await
        }
        async fn write_file(&mut self, p: &str, c: &[u8]) -> Result<(), SshError> {
            self.inner.write_file(p, c).await
        }
        async fn read_file_chunked(
            &mut self,
            p: &str,
            n: usize,
            f: &mut (dyn for<'a> FnMut(&'a [u8]) -> ChunkControl + Send),
        ) -> Result<TransferEnd, SshError> {
            self.inner.read_file_chunked(p, n, f).await
        }
        async fn write_file_chunked(
            &mut self,
            p: &str,
            n: usize,
            f: &mut (dyn for<'a> FnMut(&'a mut [u8]) -> ChunkSource + Send),
        ) -> Result<TransferEnd, SshError> {
            self.inner.write_file_chunked(p, n, f).await
        }
        async fn stat(&mut self, p: &str) -> Result<RemoteEntry, SshError> {
            self.inner.stat(p).await
        }
        async fn lstat(&mut self, p: &str) -> Result<RemoteEntry, SshError> {
            self.inner.lstat(p).await
        }
        async fn remove(&mut self, p: &str) -> Result<(), SshError> {
            self.inner.remove(p).await?;
            self.tick();
            Ok(())
        }
        async fn rename(&mut self, a: &str, b: &str) -> Result<(), SshError> {
            self.inner.rename(a, b).await
        }
        async fn posix_rename(&mut self, a: &str, b: &str) -> Result<bool, SshError> {
            self.inner.posix_rename(a, b).await
        }
        async fn create_dir(&mut self, p: &str) -> Result<(), SshError> {
            self.inner.create_dir(p).await
        }
        async fn create_file_exclusive(&mut self, p: &str) -> Result<(), SshError> {
            self.inner.create_file_exclusive(p).await
        }
        async fn remove_dir(&mut self, p: &str) -> Result<(), SshError> {
            self.inner.remove_dir(p).await?;
            self.tick();
            Ok(())
        }
        async fn set_owner(&mut self, _: &str, _: u32, _: u32) -> Result<(), SshError> {
            unreachable!()
        }
        async fn set_permissions(&mut self, p: &str, m: u32) -> Result<(), SshError> {
            self.inner.set_permissions(p, m).await?;
            self.tick();
            Ok(())
        }
    }

    fn tree(files: usize) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("big");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        for i in 0..files {
            std::fs::write(root.join(format!("f{i}.txt")), b"x").unwrap();
        }
        std::fs::write(root.join("sub/g.txt"), b"x").unwrap();
        (dir, root)
    }

    fn rows_of(emitter: &TestEmitter) -> Vec<(String, serde_json::Value)> {
        emitter.events.lock().unwrap().clone()
    }

    /// Runs `delete_recursive` the way `delete_impl_reporting` does.
    async fn delete_with_rows(
        emitter: &TestEmitter,
        sftp: &mut dyn SftpSession,
        ops: &Arc<FileOps>,
        sid: SessionId,
        guard: &crate::file_ops::OpGuard,
        path: &str,
    ) -> Result<Option<()>, SshError> {
        let _ = ops;
        let mut rows = EntryRows::new(
            Some((emitter, sid)),
            SftpTransferKind::Delete,
            guard.handle(),
            path,
        );
        let result = rows.run(delete_recursive(sftp, path, guard.handle())).await;
        rows.finish(
            result
                .as_ref()
                .map(|r| r.is_some())
                .map_err(|e| e.to_string()),
        );
        result
    }

    #[tokio::test]
    async fn test_recursive_delete_reports_monotonic_throttled_progress_with_total() {
        let (_keep, root) = tree(200);
        let emitter = TestEmitter::default();
        let ops = Arc::new(FileOps::default());
        let sid = Uuid::new_v4();
        let guard = ops.register(sid, Uuid::new_v4(), OpKind::Delete, "p", false);
        let mut sftp = LocalFileSession::new();

        delete_with_rows(
            &emitter,
            &mut sftp,
            &ops,
            sid,
            &guard,
            root.to_str().unwrap(),
        )
        .await
        .unwrap();

        let events = rows_of(&emitter);
        assert_eq!(events[0].0, "sftp-transfer-started");
        assert_eq!(events[0].1["kind"], "delete");
        let progress: Vec<_> = events
            .iter()
            .filter(|(n, _)| n == "sftp-transfer-progress")
            .map(|(_, p)| p["entries"].clone())
            .collect();
        // 201 files + 2 folders; far fewer events than entries (throttled).
        assert!(progress.len() < 20, "not throttled: {}", progress.len());
        let mut last = 0;
        for e in &progress {
            let done = e["done"].as_u64().unwrap();
            assert!(done >= last, "counts must be monotonic");
            last = done;
            if let Some(t) = e["total"].as_u64() {
                assert_eq!(t, 203);
            }
        }
        assert_eq!(progress.last().unwrap()["done"], 203);
        assert_eq!(progress.last().unwrap()["total"], 203);
        let (name, fin) = events.last().unwrap();
        assert_eq!(name, "sftp-transfer-finished");
        assert_eq!(fin["entriesDone"], 203);
        assert!(fin["error"].is_null() && fin.get("stopped").is_none());
        assert!(!root.exists());
    }

    #[tokio::test]
    async fn test_recursive_delete_cancel_stops_within_one_entry_and_reports_count() {
        let (_keep, root) = tree(20);
        let emitter = TestEmitter::default();
        let ops = Arc::new(FileOps::default());
        let sid = Uuid::new_v4();
        let op_id = Uuid::new_v4();
        let guard = ops.register(sid, op_id, OpKind::Delete, "p", false);
        let mut sftp = CancelAfter {
            inner: LocalFileSession::new(),
            ops: ops.clone(),
            session_id: sid,
            op_id,
            after: 5,
            calls: 0,
        };

        let result = delete_with_rows(
            &emitter,
            &mut sftp,
            &ops,
            sid,
            &guard,
            root.to_str().unwrap(),
        )
        .await;

        assert!(result.unwrap().is_none(), "stopped by the user");
        assert_eq!(sftp.calls, 5, "no further entry after the cancel");
        let events = rows_of(&emitter);
        let (_, fin) = events.last().unwrap();
        assert_eq!(fin["stopped"], "cancelled");
        assert_eq!(fin["entriesDone"], 5);
        assert!(root.exists(), "the rest stays");
    }

    #[cfg(unix)] // LocalFileSession cannot chmod on Windows
    #[tokio::test]
    async fn test_recursive_chmod_reports_progress_and_cancel_count() {
        let (_keep, root) = tree(10);
        let emitter = TestEmitter::default();
        let ops = Arc::new(FileOps::default());
        let sid = Uuid::new_v4();
        let op_id = Uuid::new_v4();
        let guard = ops.register(sid, op_id, OpKind::Chmod, "p", false);
        let mut sftp = CancelAfter {
            inner: LocalFileSession::new(),
            ops: ops.clone(),
            session_id: sid,
            op_id,
            after: 4,
            calls: 0,
        };
        let path = root.to_str().unwrap();
        let mut rows = EntryRows::new(
            Some((&emitter, sid)),
            SftpTransferKind::Chmod,
            guard.handle(),
            path,
        );
        let result = rows
            .run(chmod_recursive(
                &mut sftp,
                path,
                0o755,
                true,
                false,
                guard.handle(),
            ))
            .await;
        rows.finish(
            result
                .as_ref()
                .map(|r| r.is_some())
                .map_err(|e| e.to_string()),
        );

        assert!(result.unwrap().is_none());
        assert_eq!(sftp.calls, 4);
        let events = rows_of(&emitter);
        assert_eq!(events[0].1["kind"], "chmod");
        let (_, fin) = events.last().unwrap();
        assert_eq!(fin["stopped"], "cancelled");
        assert_eq!(fin["entriesDone"], 4);
        // total is the whole tree: root + sub + 11 files
        let total = events
            .iter()
            .filter_map(|(_, p)| p["entries"]["total"].as_u64())
            .next_back();
        assert_eq!(total, Some(13));
    }

    #[tokio::test]
    async fn test_single_file_delete_and_non_recursive_chmod_show_no_row() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("one.txt");
        std::fs::write(&file, b"x").unwrap();
        let emitter = TestEmitter::default();
        let ops = Arc::new(FileOps::default());
        let sid = Uuid::new_v4();
        let guard = ops.register(sid, Uuid::new_v4(), OpKind::Delete, "p", false);
        let mut sftp = LocalFileSession::new();
        let path = file.to_str().unwrap();

        delete_with_rows(&emitter, &mut sftp, &ops, sid, &guard, path)
            .await
            .unwrap();
        let mut rows = EntryRows::new(
            Some((&emitter, sid)),
            SftpTransferKind::Chmod,
            guard.handle(),
            path,
        );
        let r = rows
            .run(chmod_recursive(
                &mut sftp,
                path,
                0o600,
                false,
                false,
                guard.handle(),
            ))
            .await;
        rows.finish(r.as_ref().map(|r| r.is_some()).map_err(|e| e.to_string()));

        assert!(rows_of(&emitter).is_empty());
    }

    #[tokio::test]
    async fn test_delete_recursive_removes_nested_files_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("sub/b.txt"), b"b").unwrap();
        let mut sftp = LocalFileSession::new();

        delete_recursive(&mut sftp, root.to_str().unwrap(), &OpHandle::detached())
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

        delete_recursive(&mut sftp, file.to_str().unwrap(), &OpHandle::detached())
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
    // Windows kennt weder Symlinks noch Unix-Rechtebits über diese API
    // (ADR 0122, R1) — der Test bleibt auf Unix unverändert aktiv (Spec
    // 0054, Review des Gesamtpakets, ERHÖHTE Priorität).
    #[cfg(unix)]
    #[tokio::test]
    async fn test_delete_recursive_does_not_follow_a_symlink_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("victim.txt"), b"do not delete me").unwrap();
        let link = dir.path().join("link-to-outside");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let mut sftp = LocalFileSession::new();

        delete_recursive(&mut sftp, link.to_str().unwrap(), &OpHandle::detached())
            .await
            .expect("delete_recursive() sollte gelingen (löscht nur den Symlink)");

        assert!(!link.exists(), "der Symlink selbst sollte entfernt sein");
        assert!(
            outside.join("victim.txt").exists(),
            "das Symlink-Ziel außerhalb des Baums darf unangetastet bleiben"
        );
    }

    /// Gegenstück für rekursives chmod — dieselbe Symlink-Begründung.
    /// Unix-only wie oben (ADR 0122, R1).
    #[cfg(unix)]
    #[tokio::test]
    async fn test_chmod_recursive_does_not_follow_a_symlink_at_the_root() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, b"x").unwrap();
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
        let link = dir.path().join("link-to-outside");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let mut sftp = LocalFileSession::new();

        chmod_recursive(
            &mut sftp,
            link.to_str().unwrap(),
            0o700,
            true,
            false,
            &OpHandle::detached(),
        )
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

    // `LocalFileSession::set_permissions` liefert unter Windows absichtlich
    // `Unsupported` (keine Unix-Rechtebits über diese API) —
    // dieser Test erwartet Erfolg und bleibt deshalb wie sein Nachbar oben
    // auf Unix beschränkt (ADR 0122, R1).
    #[cfg(unix)]
    #[tokio::test]
    async fn test_chmod_recursive_without_recursive_flag_only_touches_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(&root).unwrap();
        let child = root.join("child.txt");
        std::fs::write(&child, b"x").unwrap();
        let mut sftp = LocalFileSession::new();

        chmod_recursive(
            &mut sftp,
            root.to_str().unwrap(),
            0o700,
            false,
            false,
            &OpHandle::detached(),
        )
        .await
        .unwrap();

        let root_entry = sftp.stat(root.to_str().unwrap()).await.unwrap();
        let child_entry = sftp.stat(child.to_str().unwrap()).await.unwrap();
        assert_eq!(root_entry.permissions, 0o700);
        assert_ne!(child_entry.permissions, 0o700);
    }

    // Dasselbe wie oben: `set_permissions` liefert unter Windows
    // `Unsupported`, der Test bleibt auf Unix beschränkt (ADR 0122, R1).
    #[cfg(unix)]
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
        let changed = chmod_recursive(
            &mut sftp,
            root.to_str().unwrap(),
            0o700,
            true,
            false,
            &OpHandle::detached(),
        )
        .await
        .expect("chmod_recursive() sollte gelingen");

        // Spec 0067, Teil B: Anzahl für die Sammelmeldung — tree, sub,
        // sub/child.txt.
        assert_eq!(changed, Some(3));
        let root_entry = sftp.stat(root.to_str().unwrap()).await.unwrap();
        let sub_entry = sftp.stat(root.join("sub").to_str().unwrap()).await.unwrap();
        let nested_entry = sftp.stat(nested.to_str().unwrap()).await.unwrap();
        assert_eq!(root_entry.permissions, 0o700);
        assert_eq!(sub_entry.permissions, 0o700);
        assert_eq!(nested_entry.permissions, 0o700);
    }

    #[cfg(unix)]
    fn mode_of(p: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }

    /// Issue #624: builds root(dir) / f.txt / sub(dir) / sub/g.txt.
    #[cfg(unix)]
    fn chmod_tree(root_mode: u32) -> (tempfile::TempDir, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("f.txt"), b"x").unwrap();
        std::fs::write(root.join("sub/g.txt"), b"x").unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(root_mode)).unwrap();
        (dir, root)
    }

    #[cfg(unix)]
    async fn run_chmod(root: &std::path::Path, mode: u32, special: bool) {
        let mut sftp = LocalFileSession::new();
        chmod_recursive(
            &mut sftp,
            root.to_str().unwrap(),
            mode,
            true,
            special,
            &OpHandle::detached(),
        )
        .await
        .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_chmod_recursive_644_gives_files_644_and_folders_755() {
        let (_d, root) = chmod_tree(0o755);
        run_chmod(&root, 0o644, false).await;
        assert_eq!(mode_of(&root), 0o755);
        assert_eq!(mode_of(&root.join("sub")), 0o755);
        assert_eq!(mode_of(&root.join("f.txt")), 0o644);
        assert_eq!(mode_of(&root.join("sub/g.txt")), 0o644);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_chmod_recursive_600_gives_files_600_and_folders_700() {
        let (_d, root) = chmod_tree(0o755);
        run_chmod(&root, 0o600, false).await;
        assert_eq!(mode_of(&root), 0o700);
        assert_eq!(mode_of(&root.join("sub")), 0o700);
        assert_eq!(mode_of(&root.join("f.txt")), 0o600);
        assert_eq!(mode_of(&root.join("sub/g.txt")), 0o600);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_chmod_recursive_keeps_folder_setgid_and_never_sets_it_on_files() {
        let (_d, root) = chmod_tree(0o2755);
        // The dialog started from 2755 and sends 2644 without having
        // changed the special bits.
        run_chmod(&root, 0o2644, false).await;
        assert_eq!(mode_of(&root), 0o2755);
        assert_eq!(mode_of(&root.join("sub")), 0o755);
        assert_eq!(mode_of(&root.join("f.txt")), 0o644);
        assert_eq!(mode_of(&root.join("sub/g.txt")), 0o644);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_chmod_recursive_applies_explicit_setgid_to_folders_only() {
        let (_d, root) = chmod_tree(0o755);
        run_chmod(&root, 0o2644, true).await;
        assert_eq!(mode_of(&root), 0o2755);
        assert_eq!(mode_of(&root.join("sub")), 0o2755);
        assert_eq!(mode_of(&root.join("f.txt")), 0o644);
        assert_eq!(mode_of(&root.join("sub/g.txt")), 0o644);
    }
}

/// Spec 0054, Teil 4: die von `AppState`/einer echten SFTP-Session
/// losgelösten Bausteine des "Lokal öffnen"-Flows — `sftp_open_for_editing`
/// selbst bräuchte eine volle `Session` (kein bestehendes Test-Setup dafür,
/// s. Fehlen jeglicher `sftp_*`-Command-Tests auf dieser Ebene schon vor
/// Spec 0054), aber `edit_session_dir`/`close_edit_session` sind pure bzw.
/// rein-lokale Dateisystem-Funktionen. `local_file_mtime` steht seit
/// Issue #89 (braucht Sitzung und Freigaben) in `local_path_tests`.
#[cfg(test)]
mod edit_session_tests {
    use super::*;

    /// Issue #134: every test owns its edit-copy root — a temporary
    /// directory the guard removes even when the test panics, never the
    /// real user cache.
    fn temp_edit_root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_edit_session_dir_is_distinct_per_session() {
        let root = temp_edit_root();
        let a = edit_session_dir_in(Some(root.path()), SessionId::new_v4()).unwrap();
        let b = edit_session_dir_in(Some(root.path()), SessionId::new_v4()).unwrap();
        assert_ne!(a, b);
        assert!(a.ends_with(a.file_name().unwrap()));
        assert!(a.starts_with(root.path()) && b.starts_with(root.path()));
    }

    #[test]
    fn test_edit_session_dir_without_a_cache_dir_is_an_error() {
        assert!(edit_session_dir_in(None, SessionId::new_v4()).is_err());
    }

    #[tokio::test]
    async fn test_close_edit_session_removes_the_file() {
        let root = temp_edit_root();
        let session_id = SessionId::new_v4();
        let dir = edit_session_dir_in(Some(root.path()), session_id).unwrap();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("edited.txt");
        tokio::fs::write(&path, b"x").await.unwrap();

        close_edit_session_impl(Some(root.path()), session_id, path.to_str().unwrap())
            .await
            .expect("close_edit_session() sollte gelingen");

        assert!(!path.exists());
    }

    /// Spec 0054, Teil 4, Punkt 6: "Watcher stoppt ... Temp-Datei
    /// aufräumen" — ein bereits vom Nutzer oder dem externen Programm
    /// selbst gelöschtes Temp-File ist kein Fehlerfall.
    #[tokio::test]
    async fn test_close_edit_session_on_an_already_missing_file_is_not_an_error() {
        let root = temp_edit_root();
        let session_id = SessionId::new_v4();
        let path = edit_session_dir_in(Some(root.path()), session_id)
            .unwrap()
            .join("never-existed.txt");

        close_edit_session_impl(Some(root.path()), session_id, path.to_str().unwrap())
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
        let root = temp_edit_root();
        let session_id = SessionId::new_v4();
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("not-mine.txt");
        std::fs::write(&path, b"x").unwrap();

        let result =
            close_edit_session_impl(Some(root.path()), session_id, path.to_str().unwrap()).await;

        assert!(result.is_err());
        assert!(
            path.exists(),
            "eine fremde Datei darf nicht gelöscht werden"
        );
    }

    /// Issue #134: the scope check also holds inside the same edit-copy
    /// root — the copy of another session is not "its own session dir".
    #[tokio::test]
    async fn test_close_edit_session_rejects_the_copy_of_another_session() {
        let root = temp_edit_root();
        let other_dir = edit_session_dir_in(Some(root.path()), SessionId::new_v4()).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();
        let path = other_dir.join("nginx.conf");
        std::fs::write(&path, b"x").unwrap();

        let result = close_edit_session_impl(
            Some(root.path()),
            SessionId::new_v4(),
            path.to_str().unwrap(),
        )
        .await;

        assert!(result.is_err());
        assert!(path.exists(), "die Kopie einer anderen Session bleibt");
    }
}
