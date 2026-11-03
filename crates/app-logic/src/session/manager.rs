//! `SessionManager`: bookkeeping of the running sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use ssh_manager_core::shared::ServerId;

use super::*;

/// Eine Zeile der `SessionManager::snapshot()`-Momentaufnahme (Spec 0017,
/// Abschnitt 2) — bewusst ohne `server_name`: dafür bräuchte es den
/// `ProfileStore`, den `SessionManager` absichtlich nicht kennt (reines
/// Session-Bookkeeping, keine Persistenz-Abhängigkeit). Die Auflösung auf
/// `SessionSummaryDto` (inkl. Servername) übernimmt der Aufrufer
/// (`app_shell::commands::list_sessions`).
#[derive(Debug, Clone, Copy)]
pub struct SessionSnapshotEntry {
    pub session_id: SessionId,
    pub server_id: ServerId,
    pub status: ConnectionStatus,
    pub has_pending_action: bool,
}

/// Kapselt `AppState.sessions` (Aufgabenstellung Teil 2, Punkt 1) — s.
/// Modul-Kommentar Punkt 1 zur Wahl von `std::sync::Mutex` für die äußere
/// Map.
#[derive(Default)]
pub struct SessionManager {
    sessions: StdMutex<HashMap<SessionId, Arc<Session>>>,
    /// Spec 0114 §4: number of open sessions, for the background keep-alive.
    count_tx: tokio::sync::watch::Sender<usize>,
    /// Spec 0017, Abschnitt 2: Verbindungsversuche, die aktuell auf
    /// `confirm_host_key` warten (`app_shell::commands::connect`) — diese
    /// Sessions existieren noch nicht in `sessions` (s. dortiger
    /// Kommentar zur Reihenfolge: `Session` wird erst nach erfolgreichem
    /// Aufbau eingefügt), sollen aber trotzdem in `list_sessions()`
    /// auftauchen (Status `AwaitingHostKey`), damit ein Frontend-Reload
    /// während eines offenen Host-Key-Dialogs den Tab nicht einfach
    /// verschwinden lässt.
    pending_connections: StdMutex<HashMap<SessionId, ServerId>>,
    /// Issue #242: Chat-Sitzungen, die gerade fortgesetzt (Resume) oder
    /// gelöscht werden. Schließt die Lücke zwischen "Resume gestartet" und
    /// `insert` der registrierten `Session`, in der `is_chat_session_active`
    /// noch `false` liefert. Reine synchrone Sperre, nie über ein `.await`
    /// hinweg gehalten.
    claimed_chat_sessions: StdMutex<HashMap<uuid::Uuid, ChatSessionClaimState>>,
}

#[derive(Default, Clone, Copy)]
struct ChatSessionClaimState {
    resumers: usize,
    deleting: bool,
}

/// Die Chat-Sitzung ist gerade in Benutzung (Resume läuft, ein Tab nutzt
/// sie) bzw. wird gerade gelöscht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSessionClaimError {
    /// Ein Tab nutzt die Sitzung oder ein Resume läuft gerade.
    InUse,
    /// Ein Löschvorgang für diese Sitzung läuft gerade.
    BeingDeleted,
}

/// RAII-Anspruch auf eine Chat-Sitzung; gibt sie beim `Drop` frei — auch auf
/// jedem Fehlerpfad eines Resume.
#[must_use = "dropping the claim releases it immediately"]
pub struct ChatSessionClaim<'a> {
    manager: &'a SessionManager,
    chat_session_id: uuid::Uuid,
    exclusive: bool,
}

impl Drop for ChatSessionClaim<'_> {
    fn drop(&mut self) {
        let mut claims = self.manager.claimed_chat_sessions.lock().unwrap();
        if let Some(state) = claims.get_mut(&self.chat_session_id) {
            if self.exclusive {
                state.deleting = false;
            } else {
                state.resumers = state.resumers.saturating_sub(1);
            }
            if !state.deleting && state.resumers == 0 {
                claims.remove(&self.chat_session_id);
            }
        }
    }
}

impl SessionManager {
    /// Issue #242: Anspruch eines Resume. Mehrere gleichzeitige Resumes
    /// derselben Sitzung bleiben möglich (wie bisher); nur ein laufendes
    /// Löschen schließt sie aus. Vor dem Laden der Historie nehmen, bis nach
    /// `insert` der `Session` halten.
    pub fn claim_chat_session_for_resume(
        &self,
        chat_session_id: uuid::Uuid,
    ) -> Result<ChatSessionClaim<'_>, ChatSessionClaimError> {
        let mut claims = self.claimed_chat_sessions.lock().unwrap();
        let state = claims.entry(chat_session_id).or_default();
        if state.deleting {
            return Err(ChatSessionClaimError::BeingDeleted);
        }
        state.resumers += 1;
        Ok(ChatSessionClaim {
            manager: self,
            chat_session_id,
            exclusive: false,
        })
    }

    /// Issue #242: exklusiver Anspruch für das Löschen. Scheitert mit
    /// `InUse`, wenn ein Resume läuft oder ein Tab die Sitzung nutzt;
    /// Prüfung und Anspruch gehören zusammen, der Aufrufer löscht, solange
    /// er die Rückgabe hält.
    pub async fn claim_chat_session_for_delete(
        &self,
        chat_session_id: uuid::Uuid,
    ) -> Result<ChatSessionClaim<'_>, ChatSessionClaimError> {
        {
            let mut claims = self.claimed_chat_sessions.lock().unwrap();
            let state = claims.entry(chat_session_id).or_default();
            if state.deleting {
                return Err(ChatSessionClaimError::BeingDeleted);
            }
            if state.resumers > 0 {
                return Err(ChatSessionClaimError::InUse);
            }
            state.deleting = true;
        }
        // Ab hier hält der Anspruch (Drop gibt frei, auch beim Fehler unten).
        let claim = ChatSessionClaim {
            manager: self,
            chat_session_id,
            exclusive: true,
        };
        if self.is_chat_session_active(chat_session_id).await {
            return Err(ChatSessionClaimError::InUse);
        }
        Ok(claim)
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, id: SessionId, session: Arc<Session>) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(id, session);
        self.count_tx.send_replace(sessions.len());
    }

    /// Watches the number of open sessions (Spec 0114 §4).
    pub fn subscribe_count(&self) -> tokio::sync::watch::Receiver<usize> {
        self.count_tx.subscribe()
    }

    pub fn get(&self, id: SessionId) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().get(&id).cloned()
    }

    pub fn remove(&self, id: SessionId) -> Option<Arc<Session>> {
        let mut sessions = self.sessions.lock().unwrap();
        let removed = sessions.remove(&id);
        self.count_tx.send_replace(sessions.len());
        removed
    }

    /// Spec 0040, Abschnitt 7: Grundlage für `commands::delete_chat_session`s
    /// Schutz vor dem Löschen einer gerade aktiv verbundenen Sitzung — ohne
    /// diese Prüfung würde die zugehörige `chat_sessions`-Zeile unter einer
    /// noch laufenden `Session` weggezogen, und jeder weitere `push_history`-
    /// Aufruf dieser Sitzung liefe fortan ins Leere (Fremdschlüssel-Verletzung
    /// bei jedem Schreibversuch, nur als Warnung geloggt statt sichtbar zu
    /// scheitern — s. `orchestration::push_history_scoped`). Klont nur die
    /// `Arc`-Zeiger unter der kurzen synchronen Sperre, bevor der `.await`
    /// auf `chat_session_id` je Session erfolgt — derselbe Grund wie bei
    /// `snapshot()` oben.
    pub async fn is_chat_session_active(&self, chat_session_id: uuid::Uuid) -> bool {
        let sessions: Vec<Arc<Session>> = self.sessions.lock().unwrap().values().cloned().collect();
        for session in sessions {
            if *session.persistence.chat_session_id.lock().await == Some(chat_session_id) {
                return true;
            }
        }
        false
    }

    pub fn register_pending_connection(&self, id: SessionId, server_id: ServerId) {
        self.pending_connections
            .lock()
            .unwrap()
            .insert(id, server_id);
    }

    pub fn clear_pending_connection(&self, id: SessionId) {
        self.pending_connections.lock().unwrap().remove(&id);
    }

    /// Momentaufnahme aller offenen Sessions plus aller noch auf einen
    /// Host-Key wartenden Verbindungsversuche (Spec 0017, Abschnitt 2) —
    /// Grundlage für `list_sessions()`. Jeder Feldzugriff (`status`,
    /// `pending_action`) sperrt nur den jeweils eigenen `Session`-internen
    /// Mutex kurz, nie die äußere `sessions`-Map über den ganzen Aufbau
    /// dieser Liste hinweg — bei vielen gleichzeitig offenen Sessions blockt
    /// das keinen parallelen `insert`/`get`/`remove`-Aufruf für nennenswerte
    /// Zeit.
    pub fn snapshot(&self) -> Vec<SessionSnapshotEntry> {
        let mut result: Vec<SessionSnapshotEntry> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .map(|(id, session)| SessionSnapshotEntry {
                session_id: *id,
                server_id: session.server_id,
                status: *session.conn.status.lock().unwrap(),
                // Spec 0088, A2.1: Der Tab-Indikator darf nicht daran
                // scheitern, dass irgendwo unter dieser Sperre einmal ein
                // Panic lief — sonst reißt eine einzelne vergiftete
                // Sitzung die Sitzungsliste aller Tabs mit.
                has_pending_action: crate::poison::lock_tolerating_poison(
                    &session.conn.pending_action,
                )
                .is_some(),
            })
            .collect();

        result.extend(
            self.pending_connections
                .lock()
                .unwrap()
                .iter()
                .map(|(id, server_id)| SessionSnapshotEntry {
                    session_id: *id,
                    server_id: *server_id,
                    status: ConnectionStatus::AwaitingHostKey,
                    has_pending_action: false,
                }),
        );

        result
    }
}
