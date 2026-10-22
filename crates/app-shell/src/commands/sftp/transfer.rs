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
use ssh_manager_core::ssh::{ChunkControl, ChunkSource, SftpSession, TransferEnd};

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
        let mut file = create_temp(&temp, run.channel.is_elevated())?;
        let mut io_error: Option<std::io::Error> = None;
        let mut throttle = Throttle(None);
        let end = {
            let mut guard = lock_browser_sftp(run.session, run.channel).await;
            let mut sftp = guard.sftp().await?;
            sftp.read_file_chunked(remote_path, TRANSFER_CHUNK_BYTES, &mut |chunk| {
                if run.should_stop() {
                    return ChunkControl::Stop;
                }
                if let Err(e) = file.write_all(chunk) {
                    io_error = Some(e);
                    return ChunkControl::Stop;
                }
                written += chunk.len() as u64;
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
                ChunkControl::Continue
            })
            .await?
        };
        if let Some(e) = io_error {
            return Err(e.into());
        }
        if end == TransferEnd::Completed {
            file.flush()?;
            drop(file);
            std::fs::rename(&temp, &local_path)?;
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
        if let Err(e) = std::fs::remove_file(&temp) {
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
    let file_name = file_name_of(remote_path);
    run.op.set_current_transfer(transfer_id);
    let local_for_stat = local_path.to_path_buf();
    let total_bytes = tokio::task::spawn_blocking(move || {
        std::fs::metadata(&local_for_stat).map(|m| m.len()).ok()
    })
    .await
    .unwrap_or(None);
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
    let result: CommandResult<TransferEnd> = async {
        let mut file = std::fs::File::open(local_path)?;
        let mut io_error: Option<std::io::Error> = None;
        let mut throttle = Throttle(None);
        let mut guard = lock_browser_sftp(run.session, run.channel).await;
        let elevated_user = guard.elevated_user();
        let mut sftp = guard.sftp().await?;
        partial_may_exist = true;
        let written = sftp
            .write_file_chunked(remote_path, TRANSFER_CHUNK_BYTES, &mut |buf| {
                if run.should_stop() {
                    return ChunkSource::Stop;
                }
                match file.read(buf) {
                    Ok(0) => ChunkSource::Eof,
                    Ok(n) => {
                        sent += n as u64;
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
                        ChunkSource::Data(n)
                    }
                    Err(e) => {
                        io_error = Some(e);
                        ChunkSource::Stop
                    }
                }
            })
            .await;
        audit_elevated_change(
            elevated_user.as_deref(),
            "upload",
            remote_path,
            matches!(written, Ok(TransferEnd::Completed)),
        );
        let end = written?;
        if let Some(e) = io_error {
            return Err(e.into());
        }
        Ok(end)
    }
    .await;

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
            let incomplete = remove_partial_upload(run, remote_path).await;
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
