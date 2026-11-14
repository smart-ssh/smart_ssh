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

#[cfg(test)]
use ssh_manager_core::ssh::RemoteEntry;
use ssh_manager_core::ssh::{SftpSession, SshError};

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
use app_logic::local_path_grants::{GrantedLocalPath, LocalPathGrants};

/// Window event hook (issue #89): the paths of a native file drop become local
/// path grants. They come from the window's drag-and-drop event, never from the
/// webview.
pub(crate) fn record_drag_drop<R: tauri::Runtime>(
    window: &tauri::Window<R>,
    event: &tauri::WindowEvent,
) {
    use tauri::Manager;
    if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
        if let Some(grants) = window.try_state::<LocalPathGrants>() {
            grants.record_drop(paths.clone());
        }
    }
}
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

mod list;
pub(crate) use list::*;

mod download;
pub(crate) use download::*;

mod upload_clipboard;
pub(crate) use upload_clipboard::*;

mod delete;
pub(crate) use delete::*;

mod permissions;
pub(crate) use permissions::*;

mod rename_mkdir;
pub(crate) use rename_mkdir::*;

mod copy_content;
pub(crate) use copy_content::*;

mod edit_session;
pub(crate) use edit_session::*;

#[cfg(test)]
mod tests_mutation;

#[cfg(test)]
mod tests_edit_session;
