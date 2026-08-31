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
use crate::state::SessionId;

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
}

/// Startet den Terminal-Aktor (Modul-Kommentar, Punkt 3) als eigenen Task.
/// Läuft, bis `shell.read()` EOF liefert (leerer Chunk, s.
/// `ssh_transport::RusshShell::read`), ein Lesefehler auftritt, oder der
/// Sender-Teil des Kommando-Kanals gedroppt wird (Session verworfen).
pub fn spawn_terminal_actor(
    session_id: SessionId,
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
            emit_connection_status_changed(
                emitter.as_ref(),
                session_id,
                ConnectionStatus::Disconnected,
                reason,
            );
        }
    });
}

/// Kapselt `AppState.sessions` (Aufgabenstellung Teil 2, Punkt 1) — s.
/// Modul-Kommentar Punkt 1 zur Wahl von `std::sync::Mutex` für die äußere
/// Map.
#[derive(Default)]
pub struct SessionManager {
    sessions: StdMutex<HashMap<SessionId, Arc<Session>>>,
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
}
