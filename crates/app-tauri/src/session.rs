//! Laufende Server-Sessions (Spec 0007, Abschnitt 3) und deren
//! Nebenläufigkeits-Modell.
//!
//! **Drei verschiedene Synchronisierungsmechanismen, bewusst nicht
//! einheitlich `Mutex<HashMap<...>>` überall:**
//!
//! 1. `SessionManager`s äußere Map (`SessionId -> Arc<Session>`) ist ein
//!    simples `std::sync::Mutex`: sie wird nur für kurze, nie über einen
//!    `.await`-Punkt hinweg gehaltene Zeigervorgänge (Einfügen/Nachschlagen/
//!    Entfernen) gesperrt — dafür ist eine synchrone Std-Mutex leichter und
//!    genügt völlig; `tokio::sync::Mutex` wäre hier nur unnötiger Overhead.
//! 2. `Session.transport`/`Session.context` nutzen dagegen
//!    `tokio::sync::Mutex`: beide werden über `.await`-Punkte hinweg
//!    gehalten (`SshTransport::execute()`, `AiProvider::send()`-Stream
//!    konsumieren) — eine `std::sync::MutexGuard` über einen Await-Punkt
//!    hinweg zu halten ist nicht `Send` und lässt sich in einer async
//!    Tauri-Command-Funktion nicht compilieren.
//! 3. Der interaktive Terminal-Kanal (`InteractiveShell`) bekommt **keinen**
//!    Mutex, sondern einen Aktor: ein einzelner Hintergrund-Task besitzt
//!    die `Box<dyn InteractiveShell>` exklusiv und wählt per `tokio::select!`
//!    zwischen "nächster Chunk vom Server" (`shell.read()`, kann beliebig
//!    lange blockieren) und "nächstes Kommando vom Frontend"
//!    (`terminal_input`/`terminal_resize` über einen `mpsc`-Kanal). Ein
//!    Mutex um die Shell hätte hier ein echtes Problem: `read()` "blockiert,
//!    bis Daten verfügbar sind" (s. `core::ssh::InteractiveShell`-Doc) — ein
//!    Hintergrund-Task, der den Mutex während eines laufenden `read()`-Awaits
//!    hält, würde `terminal_input`/`terminal_resize` bis zum nächsten
//!    Server-Chunk blockieren und die Eingabe spürbar verzögern. Der Aktor
//!    umgeht das vollständig, da nur er selbst je auf die Shell zugreift.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use ssh_manager_core::ai::{AiProvider, OutputRedactor, SessionContext};
use ssh_manager_core::filter::{Decision, EvalContext, FilterEngine, PolicyStore};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{PtySize, SshTransport};

use crate::events::{emit_connection_status_changed, ConnectionStatus, EventEmitter};
use crate::state::{ActionId, SessionId};

/// Objektsicherer Wrapper um `FilterEngine<S>::evaluate` (Aufgabenstellung
/// Teil 1, Punkt 4). `FilterEngine<S>` selbst ist über den `PolicyStore`-Typ
/// generisch, `Session` soll dieses Detail aber nicht nach außen tragen
/// (Tests wollen unterschiedliche `PolicyStore`-Implementierungen
/// einsetzen können, ohne `Session` selbst generisch machen zu müssen) —
/// ein kleines, dyn-kompatibles Trait löst das, analog zu `EventEmitter`.
///
/// `async fn` seit Spec 0009 (`PolicyStore::rules_for` liest jetzt aus der
/// SQLite-Datenbank).
#[async_trait]
pub trait CommandEvaluator: Send + Sync {
    async fn evaluate(&self, command: &str, ctx: &EvalContext) -> Decision;
}

#[async_trait]
impl<S: PolicyStore + Send + Sync> CommandEvaluator for FilterEngine<S> {
    async fn evaluate(&self, command: &str, ctx: &EvalContext) -> Decision {
        FilterEngine::evaluate(self, command, ctx).await
    }
}

/// Kommando an den Terminal-Aktor (s. Modul-Kommentar, Punkt 3).
pub enum TerminalCommand {
    Write(Vec<u8>),
    Resize(PtySize),
}

pub struct Session {
    pub transport: AsyncMutex<Box<dyn SshTransport>>,
    pub ai_provider: Box<dyn AiProvider>,
    pub context: AsyncMutex<SessionContext>,
    pub filter_engine: Box<dyn CommandEvaluator>,
    pub server_id: ServerId,
    pub tags: Vec<String>,
    /// `Some`, sobald `open_terminal` den Aktor gestartet hat.
    /// `terminal_input`/`terminal_resize` senden darüber; `None` (noch kein
    /// `open_terminal` aufgerufen, oder Kanal bereits geschlossen) wird als
    /// Anwenderfehler zurückgemeldet statt zu blockieren.
    pub terminal: StdMutex<Option<mpsc::UnboundedSender<TerminalCommand>>>,
    /// Läuft über jeden `SshTransport::execute()`-Output, bevor er in
    /// `chat-action-result` und `context.history` landet (Spec 0006,
    /// Abschnitt 5) — pro Session eine Instanz statt bei jeder Ausführung
    /// neu aufgebaut (baut intern eine feste Regex-Liste auf).
    pub redactor: Box<dyn OutputRedactor>,
    /// Für `NoteEditor::Ai { provider, model }` bei `ProposeNoteUpdate`
    /// (Spec 0003, Abschnitt 5.3) — aus der aktiven `AiProviderConfig` zum
    /// Verbindungszeitpunkt übernommen, damit spätere Notiz-Historie
    /// nachvollziehbar bleibt, welcher Provider/welches Modell den
    /// Vorschlag gemacht hat.
    pub ai_provider_label: String,
    pub ai_model: String,
    /// Spec 0017, Abschnitt 2: `list_sessions()`-Statusfeld. Startet bei
    /// `Connected` (`Session` wird erst nach erfolgreichem Verbindungsaufbau
    /// konstruiert, s. `crate::commands::connect`) und wird von
    /// `spawn_terminal_actor` auf `Disconnected` gesetzt, sobald der
    /// Terminal-Aktor unerwartet endet (Netzwerkfehler/Verbindungsabbruch) —
    /// ein expliziter `disconnect()`-Aufruf entfernt die Session ohnehin
    /// komplett aus `SessionManager`, dort ist kein Status-Update nötig.
    /// Eigener `StdMutex` statt z. B. eines `AtomicU8`: `ConnectionStatus`
    /// ist ein einfaches Copy-Enum, aber kein Integer — ein Mutex bleibt
    /// hier die geradlinigste Wahl, analog zu `terminal` oben.
    pub status: StdMutex<ConnectionStatus>,
    /// Spec 0017, Abschnitt 5: `Some(action_id)`, während diese Session auf
    /// eine `Confirm`-Bestätigung wartet (gesetzt in
    /// `crate::orchestration::handle_action_proposed`, gelöscht sobald die
    /// wartende `rx.await` dort zurückkehrt) — die Grundlage für
    /// `SessionSummaryDto.has_pending_action`, den Tab-Indikator im
    /// Frontend. **Nicht** für den Disconnect-Notiz-Vorschlag (Spec 0010)
    /// verwendet: der läuft bewusst erst, nachdem die Session bereits aus
    /// `SessionManager` entfernt wurde, und ist als App-weite Benachrichtigung
    /// (`note-update-suggested`) ohnehin nie an einen Tab gebunden.
    pub pending_action: StdMutex<Option<ActionId>>,
}

/// Startet den Terminal-Aktor (Modul-Kommentar, Punkt 3) als eigenen Task.
/// Läuft, bis `shell.read()` EOF liefert (leerer Chunk, s.
/// `ssh_transport::RusshShell::read`), ein Lesefehler auftritt, oder der
/// Sender-Teil des Kommando-Kanals gedroppt wird (Session verworfen).
pub fn spawn_terminal_actor(
    session_id: SessionId,
    session: Arc<Session>,
    mut shell: Box<dyn ssh_manager_core::ssh::InteractiveShell>,
    mut commands: mpsc::UnboundedReceiver<TerminalCommand>,
    emitter: Arc<dyn EventEmitter>,
) {
    tokio::spawn(async move {
        // `Some(reason)` löst am Ende ein `connection-status-changed`-Event
        // aus, `None` unterdrückt es bewusst — das passiert nur, wenn der
        // Sender absichtlich gedroppt wurde (`crate::commands::disconnect`
        // hat `session.terminal` auf `None` gesetzt), und dieser Befehl hat
        // das Event dafür bereits selbst gesendet. Ohne diese Unterscheidung
        // gäbe es bei jedem expliziten `disconnect()` zwei Events statt
        // eines (einmal vom Befehl, einmal vom hier endenden Aktor).
        let disconnect_reason: Option<Option<String>> = loop {
            tokio::select! {
                read_result = shell.read() => {
                    match read_result {
                        Ok(data) if !data.is_empty() => {
                            crate::events::emit_terminal_output(emitter.as_ref(), session_id, &data);
                        }
                        Ok(_) => break Some(None), // leerer Chunk == EOF/Close (s. InteractiveShell-Doc)
                        Err(err) => break Some(Some(err.to_string())),
                    }
                }
                cmd = commands.recv() => {
                    match cmd {
                        Some(TerminalCommand::Write(data)) => {
                            if let Err(err) = shell.write(&data).await {
                                break Some(Some(err.to_string()));
                            }
                        }
                        Some(TerminalCommand::Resize(size)) => {
                            // Ein Resize-Fehler ist nicht fatal für die
                            // laufende Sitzung (z. B. kurzzeitiger
                            // Channel-Hänger) — anders als ein Lese-/
                            // Schreibfehler kein Grund, den Aktor zu
                            // beenden.
                            if let Err(err) = shell.resize(size).await {
                                eprintln!("terminal_resize fehlgeschlagen: {err}");
                            }
                        }
                        None => break None, // absichtlich beendet, s. o.
                    }
                }
            }
        };

        if let Some(reason) = disconnect_reason {
            // Spec 0017, Abschnitt 2: unerwarteter Verbindungsabbruch (nicht
            // über den expliziten `disconnect()`-Befehl) muss sich auch in
            // `list_sessions()` widerspiegeln, solange der Tab im Frontend
            // noch offen ist — sonst zeigt eine wiederhergestellte Tab-Leiste
            // (Abschnitt 2, "Backend ist maßgebliche Quelle") eine tote
            // Session fälschlich als weiterhin verbunden an.
            *session.status.lock().unwrap() = ConnectionStatus::Disconnected;
            emit_connection_status_changed(
                emitter.as_ref(),
                session_id,
                ConnectionStatus::Disconnected,
                reason,
            );
        }
    });
}

/// Eine Zeile der `SessionManager::snapshot()`-Momentaufnahme (Spec 0017,
/// Abschnitt 2) — bewusst ohne `server_name`: dafür bräuchte es den
/// `ProfileStore`, den `SessionManager` absichtlich nicht kennt (reines
/// Session-Bookkeeping, keine Persistenz-Abhängigkeit). Die Auflösung auf
/// `SessionSummaryDto` (inkl. Servername) übernimmt der Aufrufer
/// (`crate::commands::list_sessions`).
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
    /// Spec 0017, Abschnitt 2: Verbindungsversuche, die aktuell auf
    /// `confirm_host_key` warten (`crate::commands::connect`) — diese
    /// Sessions existieren noch nicht in `sessions` (s. dortiger
    /// Kommentar zur Reihenfolge: `Session` wird erst nach erfolgreichem
    /// Aufbau eingefügt), sollen aber trotzdem in `list_sessions()`
    /// auftauchen (Status `AwaitingHostKey`), damit ein Frontend-Reload
    /// während eines offenen Host-Key-Dialogs den Tab nicht einfach
    /// verschwinden lässt.
    pending_connections: StdMutex<HashMap<SessionId, ServerId>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, id: SessionId, session: Arc<Session>) {
        self.sessions.lock().unwrap().insert(id, session);
    }

    pub fn get(&self, id: SessionId) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().get(&id).cloned()
    }

    pub fn remove(&self, id: SessionId) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().remove(&id)
    }

    pub fn register_pending_connection(&self, id: SessionId, server_id: ServerId) {
        self.pending_connections.lock().unwrap().insert(id, server_id);
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
                status: *session.status.lock().unwrap(),
                has_pending_action: session.pending_action.lock().unwrap().is_some(),
            })
            .collect();

        result.extend(self.pending_connections.lock().unwrap().iter().map(
            |(id, server_id)| SessionSnapshotEntry {
                session_id: *id,
                server_id: *server_id,
                status: ConnectionStatus::AwaitingHostKey,
                has_pending_action: false,
            },
        ));

        result
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use futures::Stream;
    use uuid::Uuid;

    use ssh_manager_core::ai::{
        default_action_schemas, AiEvent, AiProvider, DefaultOutputRedactor, SessionContext,
    };
    use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, SshError};

    use super::*;
    use crate::policy::NoRulesPolicyStore;

    /// Wird in diesen Tests nie tatsächlich aufgerufen (nur die
    /// `SessionManager`-Buchführung selbst steht hier auf dem Prüfstand,
    /// nicht der Chat-/Terminal-Ablauf, s. `crate::orchestration`s Tests
    /// dafür) — `unreachable!` statt einer stillen Fake-Antwort macht einen
    /// versehentlichen Aufruf sofort sichtbar.
    struct UnusedAiProvider;
    impl AiProvider for UnusedAiProvider {
        fn send(&self, _ctx: SessionContext) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
            unreachable!("dieser Test ruft AiProvider::send nie auf")
        }
    }

    struct UnusedTransport;
    #[async_trait]
    impl SshTransport for UnusedTransport {
        async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
            unreachable!("dieser Test ruft SshTransport::execute nie auf")
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
            unreachable!("dieser Test ruft SshTransport::open_shell nie auf")
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    fn dummy_session(server_id: ServerId) -> Session {
        Session {
            transport: AsyncMutex::new(Box::new(UnusedTransport)),
            ai_provider: Box::new(UnusedAiProvider),
            context: AsyncMutex::new(SessionContext {
                system_context: String::new(),
                history: Vec::new(),
                available_actions: default_action_schemas(),
            }),
            filter_engine: Box::new(FilterEngine::new(NoRulesPolicyStore)),
            server_id,
            tags: Vec::new(),
            terminal: StdMutex::new(None),
            redactor: Box::new(DefaultOutputRedactor::new()),
            ai_provider_label: "test-provider".to_string(),
            ai_model: "test-model".to_string(),
            status: StdMutex::new(ConnectionStatus::Connected),
            pending_action: StdMutex::new(None),
        }
    }

    #[test]
    fn test_insert_get_remove_round_trip() {
        let manager = SessionManager::new();
        let id = Uuid::new_v4();
        let server_id = ServerId::new();
        manager.insert(id, Arc::new(dummy_session(server_id)));

        assert!(manager.get(id).is_some());
        assert_eq!(manager.get(id).unwrap().server_id, server_id);

        let removed = manager.remove(id);
        assert!(removed.is_some());
        assert!(manager.get(id).is_none());
    }

    #[test]
    fn test_snapshot_reports_status_and_pending_action_per_session() {
        let manager = SessionManager::new();
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let server_a = ServerId::new();
        let server_b = ServerId::new();

        let session_a = dummy_session(server_a);
        // Simuliert eine wartende Confirm-Bestätigung (Spec 0017, Abschnitt
        // 5), wie sie `orchestration::handle_action_proposed` setzt.
        *session_a.pending_action.lock().unwrap() = Some(Uuid::new_v4());
        manager.insert(id_a, Arc::new(session_a));

        let session_b = dummy_session(server_b);
        *session_b.status.lock().unwrap() = ConnectionStatus::Disconnected;
        manager.insert(id_b, Arc::new(session_b));

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.len(), 2);

        let entry_a = snapshot.iter().find(|e| e.session_id == id_a).unwrap();
        assert_eq!(entry_a.server_id, server_a);
        assert_eq!(entry_a.status, ConnectionStatus::Connected);
        assert!(
            entry_a.has_pending_action,
            "Session mit wartender Confirm-Entscheidung muss has_pending_action=true melden"
        );

        let entry_b = snapshot.iter().find(|e| e.session_id == id_b).unwrap();
        assert_eq!(entry_b.status, ConnectionStatus::Disconnected);
        assert!(!entry_b.has_pending_action);
    }

    #[test]
    fn test_snapshot_includes_pending_host_key_connections_as_awaiting() {
        let manager = SessionManager::new();
        let pending_id = Uuid::new_v4();
        let server_id = ServerId::new();
        manager.register_pending_connection(pending_id, server_id);

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].session_id, pending_id);
        assert_eq!(snapshot[0].server_id, server_id);
        assert_eq!(snapshot[0].status, ConnectionStatus::AwaitingHostKey);
        assert!(!snapshot[0].has_pending_action);

        manager.clear_pending_connection(pending_id);
        assert!(manager.snapshot().is_empty());
    }

    #[test]
    fn test_snapshot_combines_real_sessions_and_pending_connections() {
        let manager = SessionManager::new();
        let connected_id = Uuid::new_v4();
        manager.insert(connected_id, Arc::new(dummy_session(ServerId::new())));

        let pending_id = Uuid::new_v4();
        manager.register_pending_connection(pending_id, ServerId::new());

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.iter().any(|e| e.session_id == connected_id
            && e.status == ConnectionStatus::Connected));
        assert!(snapshot
            .iter()
            .any(|e| e.session_id == pending_id && e.status == ConnectionStatus::AwaitingHostKey));
    }
}
