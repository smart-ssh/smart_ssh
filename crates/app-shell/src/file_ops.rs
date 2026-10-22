//! Issue #622: die laufenden Datei-Operationen des Dateibrowsers (Transfers,
//! rekursives Löschen, rekursives chmod) je Sitzung.
//!
//! Zwei Aufgaben:
//! * **Abbruch:** jede Operation trägt einen [`OpHandle`]. Transfers prüfen
//!   ihn vor jedem Stück, Rekursionen vor jedem Eintrag. Gesetzt wird er vom
//!   Abbrechen-Knopf, vom bestätigten Ausschalten des erhöhten Modus und vom
//!   Widerruf des Kanals (Trennen, Neu-Aktivieren).
//! * **Übersicht:** der Bestätigungsdialog beim Ausschalten listet die
//!   laufenden Operationen des erhöhten Kanals (Art, Pfad, Fortschritt).
//!
//! Die Zuordnung liegt in der [`crate::elevated_sftp::ElevatedSftpRegistry`],
//! weil jede Stelle, die einen erhöhten Kanal widerruft, dort vorbeikommt.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use uuid::Uuid;

use app_logic::dto::RunningOperationDto;
use app_logic::state::SessionId;

/// Obergrenze für die Größe eines Stücks bei Up- und Downloads (Issue #622):
/// der Speicher hält je Transfer höchstens ein Stück dieser Größe.
pub(crate) const TRANSFER_CHUNK_BYTES: usize = 256 * 1024;

/// Mindestabstand zwischen zwei Fortschrittsereignissen je Transfer
/// (höchstens ca. 4 pro Sekunde).
pub(crate) const PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(250);

/// Wie lange das bestätigte Ausschalten auf das Ende der gestoppten
/// Operationen wartet (Aufräumen: Temp-Datei, Teil-Upload), bevor der Kanal
/// trotzdem widerrufen wird.
pub(crate) const STOP_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpKind {
    Download,
    Upload,
    Delete,
    Chmod,
}

impl OpKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Upload => "upload",
            Self::Delete => "delete",
            Self::Chmod => "chmod",
        }
    }
}

/// Warum eine Operation gestoppt wurde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopReason {
    /// Abbrechen-Knopf der Zeile → „cancelled“.
    Cancelled,
    /// Bestätigter Dialog (Ausschalten/Nutzerwechsel) → „stopped“.
    Stopped,
    /// Kanal widerrufen (Trennen, Neu-Aktivieren) → „stopped“, ohne Dialog.
    Revoked,
}

impl StopReason {
    fn code(self) -> u8 {
        match self {
            Self::Cancelled => 1,
            Self::Stopped => 2,
            Self::Revoked => 3,
        }
    }
}

const UNKNOWN: u64 = u64::MAX;

struct OpState {
    id: Uuid,
    kind: OpKind,
    path: String,
    elevated: bool,
    /// 0 = läuft, sonst [`StopReason::code`]. Der erste Grund bleibt.
    stop: AtomicU8,
    bytes_done: AtomicU64,
    bytes_total: AtomicU64,
    files_done: AtomicU64,
    files_total: AtomicU64,
    /// Ordner-Transfers: die Zeile (Transfer-Kennung) der Datei, die gerade
    /// läuft — der Abbrechen-Knopf dieser Zeile stoppt den ganzen Ordner.
    current_transfer: StdMutex<Option<Uuid>>,
}

/// Geteilter Zustand **einer** Operation.
#[derive(Clone)]
pub(crate) struct OpHandle(Arc<OpState>);

impl OpHandle {
    pub(crate) fn id(&self) -> Uuid {
        self.0.id
    }

    pub(crate) fn stop_reason(&self) -> Option<StopReason> {
        match self.0.stop.load(Ordering::SeqCst) {
            1 => Some(StopReason::Cancelled),
            2 => Some(StopReason::Stopped),
            3 => Some(StopReason::Revoked),
            _ => None,
        }
    }

    /// Vom Nutzer gestoppt (Knopf oder bestätigter Dialog) — anders als ein
    /// Widerruf, der an der nächsten Kanal-Operation mit der bekannten
    /// Meldung scheitert (Spec 0085, A1.2).
    pub(crate) fn is_user_stopped(&self) -> bool {
        matches!(
            self.stop_reason(),
            Some(StopReason::Cancelled | StopReason::Stopped)
        )
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.0.stop.load(Ordering::SeqCst) != 0
    }

    fn request_stop(&self, reason: StopReason) {
        let _ = self
            .0
            .stop
            .compare_exchange(0, reason.code(), Ordering::SeqCst, Ordering::SeqCst);
    }

    pub(crate) fn set_bytes_done(&self, n: u64) {
        self.0.bytes_done.store(n, Ordering::SeqCst);
    }

    pub(crate) fn bytes_done(&self) -> u64 {
        self.0.bytes_done.load(Ordering::SeqCst)
    }

    pub(crate) fn set_bytes_total(&self, n: Option<u64>) {
        self.0
            .bytes_total
            .store(n.unwrap_or(UNKNOWN), Ordering::SeqCst);
    }

    pub(crate) fn set_files(&self, done: u64, total: Option<u64>) {
        self.0.files_done.store(done, Ordering::SeqCst);
        self.0
            .files_total
            .store(total.unwrap_or(UNKNOWN), Ordering::SeqCst);
    }

    /// Eine Operation ohne Eintrag in der Zuordnung — für Tests und
    /// Aufrufer, die nicht abbrechbar sein müssen.
    #[cfg(test)]
    pub(crate) fn detached() -> Self {
        Self(Arc::new(OpState {
            id: Uuid::new_v4(),
            kind: OpKind::Delete,
            path: String::new(),
            elevated: false,
            stop: AtomicU8::new(0),
            bytes_done: AtomicU64::new(0),
            bytes_total: AtomicU64::new(UNKNOWN),
            files_done: AtomicU64::new(0),
            files_total: AtomicU64::new(UNKNOWN),
            current_transfer: StdMutex::new(None),
        }))
    }

    pub(crate) fn set_current_transfer(&self, id: Uuid) {
        *self.0.current_transfer.lock().unwrap() = Some(id);
    }

    pub(crate) fn add_files_done(&self) {
        self.0.files_done.fetch_add(1, Ordering::SeqCst);
    }

    /// Gesamtfortschritt eines Ordner-Transfers, sobald Dateizahl und
    /// Gesamtgröße (nach dem Durchlaufen des Baums) bekannt sind.
    pub(crate) fn folder_progress(&self) -> Option<app_logic::events::SftpFolderProgress> {
        let files_total = self.0.files_total.load(Ordering::SeqCst);
        let bytes_total = self.0.bytes_total.load(Ordering::SeqCst);
        (files_total != UNKNOWN && bytes_total != UNKNOWN).then(|| {
            app_logic::events::SftpFolderProgress {
                files_done: self.0.files_done.load(Ordering::SeqCst),
                files_total,
                bytes_done: self.bytes_done(),
                bytes_total,
            }
        })
    }

    fn dto(&self) -> RunningOperationDto {
        let opt = |a: &AtomicU64| match a.load(Ordering::SeqCst) {
            UNKNOWN => None,
            n => Some(n),
        };
        let total = opt(&self.0.bytes_total);
        let files_total = opt(&self.0.files_total);
        RunningOperationDto {
            id: self.0.id.to_string(),
            kind: self.0.kind.as_str(),
            path: self.0.path.clone(),
            bytes_done: total.map(|_| self.bytes_done()),
            bytes_total: total,
            files_done: files_total.map(|_| self.0.files_done.load(Ordering::SeqCst)),
            files_total,
        }
    }
}

#[derive(Default)]
pub(crate) struct FileOps {
    ops: StdMutex<HashMap<SessionId, Vec<OpHandle>>>,
    ended: tokio::sync::Notify,
}

/// Trägt eine Operation ein und nimmt sie beim Drop wieder heraus.
pub(crate) struct OpGuard {
    registry: Arc<FileOps>,
    session_id: SessionId,
    handle: OpHandle,
}

impl OpGuard {
    pub(crate) fn handle(&self) -> &OpHandle {
        &self.handle
    }
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        let mut ops = self.registry.ops.lock().unwrap();
        if let Some(list) = ops.get_mut(&self.session_id) {
            list.retain(|h| h.id() != self.handle.id());
            if list.is_empty() {
                ops.remove(&self.session_id);
            }
        }
        drop(ops);
        self.registry.ended.notify_waiters();
    }
}

impl FileOps {
    pub(crate) fn register(
        self: &Arc<Self>,
        session_id: SessionId,
        id: Uuid,
        kind: OpKind,
        path: &str,
        elevated: bool,
    ) -> OpGuard {
        let handle = OpHandle(Arc::new(OpState {
            id,
            kind,
            path: path.to_string(),
            elevated,
            stop: AtomicU8::new(0),
            bytes_done: AtomicU64::new(0),
            bytes_total: AtomicU64::new(UNKNOWN),
            files_done: AtomicU64::new(0),
            files_total: AtomicU64::new(UNKNOWN),
            current_transfer: StdMutex::new(None),
        }));
        self.ops
            .lock()
            .unwrap()
            .entry(session_id)
            .or_default()
            .push(handle.clone());
        OpGuard {
            registry: self.clone(),
            session_id,
            handle,
        }
    }

    /// Abbrechen-Knopf: stoppt genau diese Operation. `false`: unbekannt
    /// (schon fertig oder anderer Sitzung).
    pub(crate) fn cancel(&self, session_id: SessionId, id: Uuid) -> bool {
        let ops = self.ops.lock().unwrap();
        match ops.get(&session_id).and_then(|l| {
            l.iter()
                .find(|h| h.id() == id || *h.0.current_transfer.lock().unwrap() == Some(id))
        }) {
            Some(h) => {
                h.request_stop(StopReason::Cancelled);
                true
            }
            None => false,
        }
    }

    /// Stoppt alle Operationen dieser Sitzung, die über den erhöhten Kanal
    /// laufen.
    pub(crate) fn stop_elevated(&self, session_id: SessionId, reason: StopReason) {
        if let Some(list) = self.ops.lock().unwrap().get(&session_id) {
            for h in list.iter().filter(|h| h.0.elevated) {
                h.request_stop(reason);
            }
        }
    }

    pub(crate) fn running_elevated(&self, session_id: SessionId) -> Vec<RunningOperationDto> {
        self.ops
            .lock()
            .unwrap()
            .get(&session_id)
            .map(|l| {
                l.iter()
                    .filter(|h| h.0.elevated)
                    .map(OpHandle::dto)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn has_elevated(&self, session_id: SessionId) -> bool {
        self.ops
            .lock()
            .unwrap()
            .get(&session_id)
            .is_some_and(|l| l.iter().any(|h| h.0.elevated))
    }

    /// Wartet, bis keine erhöhte Operation dieser Sitzung mehr läuft, höchstens
    /// `timeout`. `true`: Ruhe.
    pub(crate) async fn wait_elevated_idle(
        &self,
        session_id: SessionId,
        timeout: Duration,
    ) -> bool {
        let wait = async {
            loop {
                let notified = self.ended.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if !self.has_elevated(session_id) {
                    return;
                }
                notified.await;
            }
        };
        tokio::time::timeout(timeout, wait).await.is_ok()
    }
}
