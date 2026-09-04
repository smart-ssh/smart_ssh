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

use ssh_manager_core::ai::{
    AiProvider, ChatMessage, MessageContent, OutputRedactor, SessionContext,
};
use ssh_manager_core::filter::{Decision, EvalContext, FilterEngine, PolicyStore};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{PtySize, SftpSession, SshTransport};

use crate::confirmation::ConfirmationRegistry;
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
    /// Spec 0018, Abschnitt 6: optionales Sudo-Passwort, einmalig bei
    /// `connect()` aus dem `CredentialStore` gelesen — `None`, wenn für den
    /// Server keines hinterlegt ist (kein Fehler, s. dortiger Kommentar).
    /// Wird ausschließlich über Stdin an genau einen `sudo -S`-Aufruf
    /// weitergereicht (`crate::orchestration::execute_suggested_command`),
    /// nie auf dem Zielserver abgelegt.
    pub sudo_password: Option<secrecy::SecretString>,
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
    /// Spec 0020, Abschnitt 3: lazy geöffnet (erst beim ersten
    /// `ReadRemoteFile`/`WriteRemoteFile`/Dateibrowser-Zugriff, s.
    /// `crate::orchestration::ensure_sftp_open`), danach für die Dauer der
    /// Session offengehalten statt pro Zugriff neu aufgebaut. `AsyncMutex`
    /// wie `transport`/`context` (über `.await`-Punkte hinweg gehalten).
    pub sftp: AsyncMutex<Option<Box<dyn SftpSession>>>,
    /// Spec 0021, Abschnitt 5: gesetzt durch `crate::commands::
    /// stop_auto_continuation`, geprüft in `crate::orchestration::
    /// run_chat_turn` nur *zwischen* automatischen Folgerunden — bricht die
    /// Fortsetzungskette für die aktuelle Nutzer-Nachricht ab, lässt einen
    /// bereits offenen Bestätigungsdialog aber unangetastet (die Prüfung
    /// liegt außerhalb von `run_one_round`). Wird zu Beginn jeder neuen
    /// `run_chat_turn`-Ausführung (= jede neue Nutzer-Nachricht)
    /// zurückgesetzt. `AtomicBool` statt `StdMutex<bool>`: einfacher
    /// Flag-Zustand ohne zusammengesetzte Operationen, für den ein Mutex nur
    /// unnötigen Overhead bedeuten würde.
    pub auto_continue_stop: std::sync::atomic::AtomicBool,
    /// Spec 0026, Abschnitt 3: `Some`, wenn die optionale KI-Zweitmeinung
    /// zur Daten-Risiko-Achse aktiviert ist UND ein gültiger, separater
    /// Provider dafür konfiguriert ist — einmalig bei `connect()` aus den
    /// `tauri-plugin-store`-Einstellungen aufgelöst (analog zu
    /// `ai_provider_label`/`ai_model` oben), nicht bei jedem Aktionsvorschlag
    /// neu gelesen. Ein während einer laufenden Session geänderter Wert
    /// greift dadurch erst bei der nächsten `connect()` — ein bewusster,
    /// kleiner Scope-Kompromiss: eine Live-Aktualisierung mitten in einer
    /// Session hätte deutlich mehr Zustands-Plumbing gebraucht, für eine
    /// reine Komfort-Einstellung unverhältnismäßig.
    pub risk_second_opinion_provider: Option<Box<dyn AiProvider>>,
    /// Spec 0027: derselbe `Arc` wie `AppState.running_command_
    /// cancellations` — ein billiger Klon bei `connect()`, damit
    /// `orchestration::execute_suggested_command` (die nur `&Session`
    /// bekommt, kein `AppState`) einen laufenden Abbruch registrieren kann,
    /// ohne die Signaturen von `run_chat_turn`/`run_one_round`/
    /// `handle_action_proposed` anfassen zu müssen (s. Doc-Kommentar auf
    /// `AppState.running_command_cancellations`).
    pub running_command_cancellations: Arc<ConfirmationRegistry<ActionId, ()>>,
    /// Spec 0039, Abschnitt 5: `true`, sobald in dieser Sitzung
    /// **irgendein** durch `ai::fence_untrusted` gelaufener Inhalt in den
    /// KI-Kontext gelangt ist (Kommando-Ausgabe, SFTP-Dateiinhalt oder
    /// Server-/Gruppen-Notiz im System-Prompt) — anders als
    /// `auto_continue_stop` **niemals** zurückgesetzt, auch nicht über neue
    /// Nutzer-Nachrichten hinweg (ersetzt damit die alte, pro-Turn
    /// zurückgesetzte SEC-03-Bremse, s. `orchestration::handle_action_
    /// proposed`-Kommentar an der Eskalationsstelle für die Begründung).
    /// Initialisiert mit `true`, wenn die Sitzung bereits mit Notizen oder
    /// (künftig, Spec 0034) vorbelasteter Historie startet — s.
    /// `history_contains_untrusted_content`.
    pub untrusted_content_ingested: std::sync::atomic::AtomicBool,
    /// Spec 0039, Abschnitt 5.1: einmalig bei `connect()` vom Server-Profil
    /// übernommen (analog zu `ai_provider_label`/`risk_second_opinion_
    /// provider` oben) — steuert, wie stark eskaliert wird, NACHDEM
    /// `untrusted_content_ingested` gesetzt ist. Das Fencing selbst
    /// (Abschnitt 3/4) läuft davon unabhängig immer.
    pub post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy,
    /// Spec 0039, Abschnitt 5.2: `Some`, wenn sowohl die app-weite
    /// Zweitmeinungs-Einstellung (Spec 0026, Abschnitt 3) als auch die
    /// serverspezifische `ai_injection_check_enabled`-Einstellung aktiv
    /// sind — einmalig bei `connect()` aufgelöst (analog zu
    /// `risk_second_opinion_provider`, derselbe konfigurierte Provider,
    /// separat aufgelöst, weil `Box<dyn AiProvider>` nicht `Clone` ist).
    pub injection_check_provider: Option<Box<dyn AiProvider>>,
    /// Spec 0039, Abschnitt 5.2: `true`, sobald der letzte gelaufene
    /// Einschleusungs-Check "ja" ergeben hat — anders als
    /// `untrusted_content_ingested` NICHT dauerhaft-monoton, sondern
    /// "klebrig bis verbraucht": `handle_action_proposed` setzt es beim
    /// Eskalieren einer Aktion wieder auf `false` zurück, weil sich die
    /// Eskalation laut Spec auf "die auf diesem Inhalt basierende
    /// Folgeaktion" bezieht (Singular), nicht auf den Rest der Sitzung.
    pub injection_suspected: std::sync::atomic::AtomicBool,
    /// Spec 0034: Persistenz-Anbindung für diese Sitzung. `None` für
    /// Sitzungen, die bewusst keine `chat_sessions`-Zeile bekommen sollen
    /// (Tests; s. Abschnitt 10 zu MCP — MCP-ausgelöste Aktionen laufen
    /// ohnehin nie über `run_chat_turn`/eine eigene `Session`, s.
    /// `crate::mcp_backend`, insofern betrifft dieses Feld sie gar nicht
    /// erst). Konkreter Store-Typ statt Trait-Abstraktion — derselbe
    /// Präzedenzfall wie `SqlitePromptHistoryStore` in `AppState` (Spec
    /// 0015): kein `core`-Trait für diese Art Hilfs-Store, anders als
    /// `ProfileStore`/`PolicyStore`/`AiProvider`.
    pub chat_session_store: Option<persistence_sqlite::SqliteChatSessionStore>,
    /// Die `chat_sessions.id`-Zeile dieser laufenden Sitzung — `None`, bis
    /// `crate::commands::connect_session` sie anlegt (bzw. bei
    /// `resume_chat_session`, Teil 2, auf die wiederverwendete Zeile
    /// gesetzt wird). `AsyncMutex` statt `StdMutex`, weil
    /// `history_push_and_persist` sie über einen `.await`-Punkt hinweg
    /// hält (der eigentliche `INSERT`).
    pub chat_session_id: AsyncMutex<Option<uuid::Uuid>>,
}

/// Spec 0039, Abschnitt 5: "Bei Session Resume mit vorbelasteter Historie
/// startet die Sitzung mit `true`." Es gibt aktuell **keinen** Resume-Pfad
/// (`docs/specs/0034-chat-session-persistence.md` ist noch Entwurf,
/// `SessionContext.history` startet in `crate::commands::connect` immer
/// mit `Vec::new()`) — diese Funktion ist die dafür vorbereitete Prüfung,
/// heute aber faktisch immer mit einer leeren Historie aufgerufen.
///
/// `MessageContent::CommandResult` zählt IMMER als untrusted-Inhalt: das
/// darin gehaltene `CommandOutput` wird erst beim tatsächlichen Versand an
/// den KI-Provider über `ai::fence_untrusted` in `<stdout>`/`<stderr>`-Tags
/// gepackt (`ai-providers::{anthropic,openai_compatible}::
/// format_command_result`) — im in-memory `ChatMessage` selbst steht noch
/// kein Fence-Text, ein reiner String-Scan würde solche Einträge sonst
/// übersehen. `MessageContent::Text` (SFTP-Dateiinhalt, ggf. künftig
/// weitere Fälle) wird dagegen bereits VOR dem Push in die Historie
/// gefenced (s. `orchestration::execute_read_remote_file`), erkennbar am
/// literalen Tag-Text.
pub(crate) fn history_contains_untrusted_content(history: &[ChatMessage]) -> bool {
    const FENCE_OPEN_TAGS: [&str; 4] = ["<stdout>", "<stderr>", "<remote_file>", "<server_note>"];
    history.iter().any(|message| match &message.content {
        MessageContent::CommandResult { .. } => true,
        MessageContent::Text(text) => FENCE_OPEN_TAGS.iter().any(|tag| text.contains(tag)),
        // Kommando/Grund stammen von der KI selbst bzw. der lokalen
        // Filter-Engine, nie vom Remote-Server (s. `format_action_
        // rejected`-Doc-Kommentar in `ai-providers`) — keine untrusted
        // Quelle.
        MessageContent::ActionRejected { .. } => false,
    })
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
            if *session.chat_session_id.lock().await == Some(chat_session_id) {
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
                status: *session.status.lock().unwrap(),
                has_pending_action: session.pending_action.lock().unwrap().is_some(),
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

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use futures::Stream;
    use uuid::Uuid;

    use ssh_manager_core::ai::{
        default_action_schemas, AiEvent, AiProvider, DefaultOutputRedactor, RejectionReason, Role,
        SessionContext,
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
            sudo_password: None,
            status: StdMutex::new(ConnectionStatus::Connected),
            pending_action: StdMutex::new(None),
            sftp: AsyncMutex::new(None),
            auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
            risk_second_opinion_provider: None,
            running_command_cancellations: Arc::new(ConfirmationRegistry::new()),
            untrusted_content_ingested: std::sync::atomic::AtomicBool::new(false),
            post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
            injection_check_provider: None,
            injection_suspected: std::sync::atomic::AtomicBool::new(false),
            chat_session_store: None,
            chat_session_id: AsyncMutex::new(None),
        }
    }

    // --- Spec 0039, Abschnitt 5: history_contains_untrusted_content -------

    #[test]
    fn test_history_contains_untrusted_content_false_for_empty_history() {
        assert!(!history_contains_untrusted_content(&[]));
    }

    #[test]
    fn test_history_contains_untrusted_content_false_for_plain_text_only() {
        let history = vec![ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Wie geht es dir?".to_string()),
        }];
        assert!(!history_contains_untrusted_content(&history));
    }

    #[test]
    fn test_history_contains_untrusted_content_true_for_command_result() {
        let history = vec![ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command: "ls -la".to_string(),
                output: CommandOutput {
                    stdout: b"total 0".to_vec(),
                    stderr: Vec::new(),
                    exit_code: Some(0),
                },
                cancelled: false,
            },
        }];
        assert!(
            history_contains_untrusted_content(&history),
            "CommandResult wird beim Versand gefenced (ai-providers::format_command_result), \
             zählt also schon hier als untrusted-Inhalt"
        );
    }

    #[test]
    fn test_history_contains_untrusted_content_true_for_fenced_remote_file_text() {
        let history = vec![ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::Text(
                "Inhalt von '/etc/hosts':\n\n<remote_file>\n<source>/etc/hosts</source>\n\
                 127.0.0.1 localhost\n</remote_file>"
                    .to_string(),
            ),
        }];
        assert!(history_contains_untrusted_content(&history));
    }

    #[test]
    fn test_history_contains_untrusted_content_false_for_action_rejected() {
        let history = vec![ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::ActionRejected {
                command: "rm -rf /".to_string(),
                reason: RejectionReason::User,
            },
        }];
        assert!(
            !history_contains_untrusted_content(&history),
            "Kommando/Grund stammen von der KI/lokalen Filter-Engine, nie vom Remote-Server"
        );
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
        assert!(snapshot
            .iter()
            .any(|e| e.session_id == connected_id && e.status == ConnectionStatus::Connected));
        assert!(snapshot
            .iter()
            .any(|e| e.session_id == pending_id && e.status == ConnectionStatus::AwaitingHostKey));
    }

    // --- Spec 0040, Abschnitt 7: is_chat_session_active ---------------------

    #[tokio::test]
    async fn test_is_chat_session_active_true_for_a_live_session_bound_to_it() {
        let manager = SessionManager::new();
        let chat_session_id = Uuid::new_v4();
        let mut session = dummy_session(ServerId::new());
        session.chat_session_id = AsyncMutex::new(Some(chat_session_id));
        manager.insert(Uuid::new_v4(), Arc::new(session));

        assert!(manager.is_chat_session_active(chat_session_id).await);
    }

    #[tokio::test]
    async fn test_is_chat_session_active_false_for_an_unrelated_chat_session_id() {
        let manager = SessionManager::new();
        let mut session = dummy_session(ServerId::new());
        session.chat_session_id = AsyncMutex::new(Some(Uuid::new_v4()));
        manager.insert(Uuid::new_v4(), Arc::new(session));

        assert!(!manager.is_chat_session_active(Uuid::new_v4()).await);
    }

    #[tokio::test]
    async fn test_is_chat_session_active_false_when_no_live_session_has_a_chat_session_id() {
        let manager = SessionManager::new();
        manager.insert(Uuid::new_v4(), Arc::new(dummy_session(ServerId::new())));

        assert!(!manager.is_chat_session_active(Uuid::new_v4()).await);
    }
}
