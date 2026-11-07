//! Issue #622: Up- und Downloads in Stücken, mit Fortschritt und Abbruch.
//!
//! * Der Speicher hält je Transfer höchstens ein Stück
//!   ([`TRANSFER_CHUNK_BYTES`]) — nie die ganze Datei.
//! * Ein Download schreibt in eine Temp-Datei neben dem Ziel und benennt sie
//!   erst nach dem Erfolg um; eine vorhandene Datei gleichen Namens wird
//!   erst dann ersetzt. Bei Abbruch oder Fehler wird die Temp-Datei gelöscht.
//! * Vor **jedem** Stück wird geprüft, ob die Operation gestoppt oder der
//!   erhöhte Kanal widerrufen wurde; danach wird kein weiteres Stück mehr
//!   verarbeitet.
//! * Ein Abbruch ist kein Kanalfehler: die Zeile bekommt `cancelled`
//!   (Abbrechen-Knopf) bzw. `stopped` (bestätigtes Ausschalten, Widerruf).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use uuid::Uuid;

use app_logic::error::{CommandError, CommandResult};
use app_logic::events::{
    emit_sftp_transfer_finished, emit_sftp_transfer_progress, emit_sftp_transfer_started,
    emit_sftp_transfer_stopped, EventEmitter, SftpTransferKind,
};
use app_logic::session::Session;
use app_logic::state::SessionId;
use ssh_manager_core::ssh::{SftpSession, TransferEnd};

use crate::commands::elevation::{
    audit_elevated_change, file_name_of, lock_browser_sftp, BrowserChannel,
};
use crate::file_ops::{OpHandle, StopReason, PROGRESS_MIN_INTERVAL, TRANSFER_CHUNK_BYTES};

/// Stabiler Fehlercode: der Befehl endete, weil die Operation gestoppt wurde.
/// Das Frontend zeigt dafür keinen Fehler (die Zeile trägt den Zustand).
pub(crate) const FILE_OPERATION_STOPPED: &str = "FILE_OPERATION_STOPPED";

pub(super) fn stopped_error() -> CommandError {
    CommandError::with_code("Die Operation wurde gestoppt.", FILE_OPERATION_STOPPED)
}

fn stop_label(reason: Option<StopReason>) -> &'static str {
    match reason {
        Some(StopReason::Cancelled) => "cancelled",
        _ => "stopped",
    }
}

/// Alles, was ein einzelner Datei-Transfer von seinem Befehl braucht.
pub(super) struct FileRun<'a> {
    pub emitter: &'a dyn EventEmitter,
    pub session: &'a Session,
    pub channel: &'a BrowserChannel,
    pub session_id: SessionId,
    /// Die Operation, zu der dieser Transfer gehört (bei Ordnern: der ganze
    /// Ordner).
    pub op: &'a OpHandle,
    /// Ordner-Transfer: Fortschrittsereignisse tragen den Gesamtfortschritt.
    pub folder: bool,
}

impl FileRun<'_> {
    /// Gilt die Operation als gestoppt? Gestoppt per Knopf/Dialog oder weil
    /// der erhöhte Kanal widerrufen wurde.
    fn should_stop(&self) -> bool {
        self.op.is_stopped() || self.channel.is_revoked()
    }

    fn stop_reason(&self) -> Option<StopReason> {
        self.op
            .stop_reason()
            .or_else(|| self.channel.is_revoked().then_some(StopReason::Revoked))
    }
}

/// Höchstens ca. 4 Fortschrittsereignisse pro Sekunde je Transfer.
struct Throttle(Option<Instant>);

impl Throttle {
    fn due(&mut self) -> bool {
        let now = Instant::now();
        match self.0 {
            Some(last) if now.duration_since(last) < PROGRESS_MIN_INTERVAL => false,
            _ => {
                self.0 = Some(now);
                true
            }
        }
    }
}

/// Issue #921: local file I/O runs on the blocking pool, never as blocking
/// `std::fs` on an async worker.
pub(super) async fn blocking_io<T: Send + 'static>(
    f: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(std::io::Error::other)?
}

/// `<ziel>.part-<zufall>` im selben Ordner (gleiches Dateisystem → Umbenennen
/// ist atomar).
fn temp_path_for(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".part-{}", Uuid::new_v4().simple()));
    target.with_file_name(name)
}

fn create_temp(path: &Path, owner_only: bool) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::OpenOptionsExt;
        // Spec 0067: im erhöhten Modus können das Root-Dateien sein — lokal
        // nur für den eigenen Nutzer lesbar anlegen.
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = owner_only;
    options.open(path)
}

/// Lädt genau eine Remote-Datei in Stücken nach `local_path`. Liefert die
/// übertragenen Bytes. `base_bytes`: bereits fertige Bytes früherer Dateien
/// desselben Ordners (für den Gesamtfortschritt).
///
/// Ein Stopp endet mit [`stopped_error`] — die Zeile hat dann schon
/// `cancelled`/`stopped` bekommen.
pub(super) async fn download_file(
    run: &FileRun<'_>,
    transfer_id: Uuid,
    remote_path: &str,
    local_path: PathBuf,
    total_bytes: Option<u64>,
    base_bytes: u64,
) -> CommandResult<u64> {
    let _activity = app_logic::background_keepalive::background_activity();
    let file_name = file_name_of(remote_path);
    run.op.set_current_transfer(transfer_id);
    emit_sftp_transfer_started(
        run.emitter,
        run.session_id,
        transfer_id,
        SftpTransferKind::Download,
        file_name,
        total_bytes,
    );

    let temp = temp_path_for(&local_path);
    let mut written = 0u64;
    let result: CommandResult<TransferEnd> = async {
        let owner_only = run.channel.is_elevated();
        let temp_for_create = temp.clone();
        let mut file = blocking_io(move || create_temp(&temp_for_create, owner_only)).await?;
        let mut throttle = Throttle(None);
        // Issue #921: the session's SFTP lock is taken per chunk and released
        // before the chunk is written locally, so other operations of the
        // session (listing, previews, AI file reads) are not queued behind
        // the whole transfer. Every re-acquire re-checks the channel (the
        // guard's `sftp()` repeats the revocation/identity checks).
        let end = loop {
            if run.should_stop() {
                break TransferEnd::Stopped;
            }
            let chunk = {
                let mut guard = lock_browser_sftp(run.session, run.channel).await;
                let mut sftp = guard.sftp().await?;
                sftp.read_range(remote_path, written, TRANSFER_CHUNK_BYTES)
                    .await?
            };
            if chunk.is_empty() {
                break TransferEnd::Completed;
            }
            // Revoked or stopped while the chunk was in flight: drop it.
            if run.should_stop() {
                break TransferEnd::Stopped;
            }
            let len = chunk.len() as u64;
            file = blocking_io(move || {
                let mut file = file;
                file.write_all(&chunk)?;
                Ok(file)
            })
            .await?;
            written += len;
            run.op.set_bytes_done(base_bytes + written);
            if throttle.due() {
                emit_sftp_transfer_progress(
                    run.emitter,
                    run.session_id,
                    transfer_id,
                    written,
                    total_bytes,
                    run.folder.then(|| run.op.folder_progress()).flatten(),
                );
            }
        };
        if end == TransferEnd::Completed {
            let temp = temp.clone();
            let target = local_path.clone();
            blocking_io(move || {
                let mut file = file;
                file.flush()?;
                drop(file);
                std::fs::rename(&temp, &target)
            })
            .await?;
        }
        Ok(end)
    }
    .await;

    // Widerruf mitten im Transfer: wie an jeder anderen Operation ein Fehler
    // mit der bekannten Meldung, kein „stopped“.
    let result = match result {
        Ok(TransferEnd::Stopped) if run.stop_reason() == Some(StopReason::Revoked) => {
            Err(run.channel.revoked_error())
        }
        other => other,
    };

    // Temp-Datei weg, wenn sie nicht zur fertigen Datei wurde.
    if !matches!(result, Ok(TransferEnd::Completed)) {
        let temp = temp.clone();
        if let Err(e) = blocking_io(move || std::fs::remove_file(&temp)).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %e, "could not remove partial download file");
            }
        }
    }

    match result {
        Ok(TransferEnd::Completed) => {
            emit_sftp_transfer_finished(run.emitter, run.session_id, transfer_id, None);
            Ok(written)
        }
        Ok(TransferEnd::Stopped) => {
            emit_sftp_transfer_stopped(
                run.emitter,
                run.session_id,
                transfer_id,
                stop_label(run.stop_reason()),
                None,
            );
            Err(stopped_error())
        }
        Err(err) => {
            // Gestoppt (Knopf/Dialog) und dabei am Kanal gescheitert, bevor
            // ein Stück kam: ein Abbruch, kein Kanalfehler.
            if matches!(
                run.op.stop_reason(),
                Some(StopReason::Cancelled | StopReason::Stopped)
            ) {
                emit_sftp_transfer_stopped(
                    run.emitter,
                    run.session_id,
                    transfer_id,
                    stop_label(run.op.stop_reason()),
                    None,
                );
                return Err(stopped_error());
            }
            // Spec 0086, A2.1: derselbe Text wie im Befehlsergebnis, wenn der
            // erhöhte Modus mitten im Transfer widerrufen wurde.
            emit_sftp_transfer_finished(
                run.emitter,
                run.session_id,
                transfer_id,
                Some(run.channel.transfer_error_message(&err)),
            );
            Err(err)
        }
    }
}

/// Lädt `local_path` in Stücken nach `remote_path` hoch (Quelle ist ein
/// bereits geprüfter, freigegebener lokaler Pfad). Bei einem Stopp wird die
/// Teildatei auf dem Server entfernt; gelingt das nicht, nennt die Zeile ihren
/// Pfad.
pub(super) async fn upload_file(
    run: &FileRun<'_>,
    transfer_id: Uuid,
    local_path: &Path,
    remote_path: &str,
    base_bytes: u64,
) -> CommandResult<u64> {
    let _activity = app_logic::background_keepalive::background_activity();
    let file_name = file_name_of(remote_path);
    run.op.set_current_transfer(transfer_id);
    let local_for_stat = local_path.to_path_buf();
    let total_bytes = blocking_io(move || std::fs::metadata(&local_for_stat).map(|m| m.len()))
        .await
        .ok();
    emit_sftp_transfer_started(
        run.emitter,
        run.session_id,
        transfer_id,
        SftpTransferKind::Upload,
        file_name,
        total_bytes,
    );

    let mut sent = 0u64;
    let mut partial_may_exist = false;
    let mut elevated_user = None;
    let result: CommandResult<TransferEnd> = async {
        let local = local_path.to_path_buf();
        let mut file = blocking_io(move || std::fs::File::open(local)).await?;
        let mut throttle = Throttle(None);
        let mut first = true;
        // Issue #921: like the download, the SFTP lock is taken per chunk.
        // The first chunk (also for an empty file) creates or truncates the
        // remote file; later chunks write at the running offset.
        let end = loop {
            if run.should_stop() {
                break TransferEnd::Stopped;
            }
            let (f, buf) = blocking_io(move || {
                let mut file = file;
                let mut buf = vec![0u8; TRANSFER_CHUNK_BYTES];
                let mut filled = 0;
                while filled < buf.len() {
                    let n = file.read(&mut buf[filled..])?;
                    if n == 0 {
                        break;
                    }
                    filled += n;
                }
                buf.truncate(filled);
                Ok((file, buf))
            })
            .await?;
            file = f;
            if buf.is_empty() && !first {
                break TransferEnd::Completed;
            }
            // Revoked or stopped while the chunk was read: send nothing more.
            if run.should_stop() {
                break TransferEnd::Stopped;
            }
            {
                let mut guard = lock_browser_sftp(run.session, run.channel).await;
                elevated_user = guard.elevated_user();
                let mut sftp = guard.sftp().await?;
                partial_may_exist = true;
                sftp.write_range(remote_path, sent, &buf, first).await?;
            }
            first = false;
            if buf.is_empty() {
                break TransferEnd::Completed;
            }
            sent += buf.len() as u64;
            run.op.set_bytes_done(base_bytes + sent);
            if throttle.due() {
                emit_sftp_transfer_progress(
                    run.emitter,
                    run.session_id,
                    transfer_id,
                    sent,
                    total_bytes,
                    run.folder.then(|| run.op.folder_progress()).flatten(),
                );
            }
        };
        Ok(end)
    }
    .await;
    if partial_may_exist {
        audit_elevated_change(
            elevated_user.as_ref(),
            "upload",
            remote_path,
            matches!(result, Ok(TransferEnd::Completed)),
        );
    }

    let result = match result {
        Ok(TransferEnd::Stopped) if run.stop_reason() == Some(StopReason::Revoked) => {
            Err(run.channel.revoked_error())
        }
        other => other,
    };

    match result {
        Ok(TransferEnd::Completed) => {
            emit_sftp_transfer_finished(run.emitter, run.session_id, transfer_id, None);
            Ok(sent)
        }
        Ok(TransferEnd::Stopped) => {
            let incomplete = if partial_may_exist {
                remove_partial_upload(run, remote_path).await
            } else {
                None
            };
            emit_sftp_transfer_stopped(
                run.emitter,
                run.session_id,
                transfer_id,
                stop_label(run.stop_reason()),
                incomplete,
            );
            Err(stopped_error())
        }
        Err(err) => {
            if partial_may_exist
                && matches!(
                    run.op.stop_reason(),
                    Some(StopReason::Cancelled | StopReason::Stopped)
                )
            {
                let incomplete = remove_partial_upload(run, remote_path).await;
                emit_sftp_transfer_stopped(
                    run.emitter,
                    run.session_id,
                    transfer_id,
                    stop_label(run.op.stop_reason()),
                    incomplete,
                );
                return Err(stopped_error());
            }
            emit_sftp_transfer_finished(
                run.emitter,
                run.session_id,
                transfer_id,
                Some(run.channel.transfer_error_message(&err)),
            );
            Err(err)
        }
    }
}

/// Entfernt die Teildatei eines gestoppten Uploads. `Some(pfad)`: das ging
/// nicht (z. B. weil der erhöhte Kanal schon widerrufen ist) — die Zeile
/// nennt den unvollständigen Pfad.
async fn remove_partial_upload(run: &FileRun<'_>, remote_path: &str) -> Option<String> {
    let mut guard = lock_browser_sftp(run.session, run.channel).await;
    let removed = match guard.sftp().await {
        Ok(mut sftp) => sftp.remove(remote_path).await.map_err(|e| e.to_string()),
        Err(e) => Err(e.message),
    };
    match removed {
        Ok(()) => None,
        Err(reason) => {
            tracing::warn!(%reason, "could not remove partial upload");
            Some(remote_path.to_string())
        }
    }
}
