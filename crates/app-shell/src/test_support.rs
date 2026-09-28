//! Spec 0084, A1/A5: Testhilfen, die bewusst NICHT hinter dem
//! `test-support`-Feature von `app-logic` liegen, sondern hier in
//! `app-shell` bleiben.
//!
//! `elevation` baut `BrowserAccess`-Werte (nur in `crate::commands::
//! elevation` konstruierbar, s. dortiger Kommentar) und Fixtures für den
//! erhöhten Dateibrowser-Kanal — ein Nachbau in `app-logic` würde genau die
//! Garantie unterlaufen, die Spec 0067 A herstellt ("KI und MCP erreichen
//! den erhöhten Kanal nie", Spec 0084 A1: "`app-logic` enthält keinen Typ,
//! keine Funktion und kein Feld, über das der erhöhte Kanal erreichbar
//! ist"). `MockAiProvider` steht ebenfalls hier, weil es außer von
//! `commands::ai_providers`s eigenem Test von nichts sonst gebraucht wird
//! (anders als die geteilten Doubles in `app_logic::test_support`, die
//! `app-logic`s eigene Module UND `app-shell` brauchen).

/// Spec 0050, Fund 3: fest verdrahteter Event-Stream, unabhängig vom
/// übergebenen `SessionContext` — reicht für
/// `commands::classify_credential_test_result`, das nur das **erste**
/// Event auswertet (s. dortiger Doc-Kommentar). `ssh_manager_core::ai`
/// hat mit `MockAiProvider` (`crates/core/src/ai/tests.rs`) bereits ein
/// Äquivalent, das aber modul-privat ist (nur für Cores eigene Tests
/// gedacht) — dieselbe, hier lokal wiederholte Minimal-Implementierung
/// statt eines öffentlichen Exports nur für einen einzigen Testfall in
/// einer anderen Crate.
pub struct MockAiProvider {
    events: Vec<ssh_manager_core::ai::AiEvent>,
}

impl MockAiProvider {
    pub fn new(events: Vec<ssh_manager_core::ai::AiEvent>) -> Self {
        Self { events }
    }
}

impl ssh_manager_core::ai::AiProvider for MockAiProvider {
    fn send(
        &self,
        _context: ssh_manager_core::ai::SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = ssh_manager_core::ai::AiEvent> + Send>> {
        Box::pin(futures::stream::iter(self.events.clone()))
    }
}

/// Spec 0067/0084: geteilte Test-Bausteine für den erhöhten
/// Dateibrowser-Kanal — Sitzung, `SessionManager` und Zuordnung gehören
/// dafür zusammen (Spec 0084, A1: der Kanal hängt nicht mehr am
/// `Session`-Wert). Geteilt statt je Testmodul neu, weil sowohl
/// `crate::elevated_sftp` (Ein-/Ausschalten, A2) als auch
/// `crate::commands::elevation` (Kanalwahl, Widerruf beim Zugriff) denselben
/// Aufbau brauchen — dieselbe Begründung wie beim Moduldoc oben.
pub(crate) mod elevation {
    use std::sync::{Arc, Mutex as StdMutex};

    use async_trait::async_trait;
    use ssh_manager_core::ssh::mock::MockSftpSession;
    use ssh_manager_core::ssh::{
        CommandOutput, InteractiveShell, PtySize, SftpSession, SshError, SshTransport,
    };

    use app_logic::session::{Session, SessionManager};
    use app_logic::state::SessionId;

    use crate::commands::BrowserAccess;
    use crate::elevated_sftp::{ElevatedSftpRegistry, ElevationContext};

    pub(crate) const PATH: &str = "/usr/lib/openssh/sftp-server";

    pub(crate) fn access() -> BrowserAccess {
        BrowserAccess::for_tests()
    }

    /// Beantwortet die Probe-Kommandos nach Präfix und zeichnet alle
    /// Kommandos sowie Exec-SFTP-Starts auf.
    pub(crate) struct ProbeTransport {
        probe: CommandOutput,
        sudo_check: CommandOutput,
        pub(crate) start_fails: bool,
        log: Arc<StdMutex<Vec<String>>>,
        /// Spec 0084, T8/T8b: meldet, dass das Öffnen des erhöhten Kanals
        /// erreicht ist — also alle Proben durch sind und nur noch der
        /// Eintrag in die Zuordnung fehlt.
        pub(crate) open_reached: Option<Arc<tokio::sync::Notify>>,
        /// Spec 0084, T8/T8b: hält das Öffnen dort an, bis der Test
        /// freigibt.
        pub(crate) open_gate: Option<Arc<tokio::sync::Notify>>,
        /// Spec 0085, T4: die Kanäle, die dieser Transport der Reihe nach
        /// herausgibt — nötig, wenn ein Test ein **Neu-Aktivieren** über den
        /// echten Weg fahren will und dabei den ersten Kanal weiter
        /// beobachtet (er darf danach keine Operation mehr sehen) und den
        /// zweiten ebenfalls (er darf gar keine sehen). Ist die Reihe leer,
        /// kommt wie bisher ein frisches `MockSftpSession`.
        prepared_channels: Arc<StdMutex<std::collections::VecDeque<Box<dyn SftpSession>>>>,
    }

    pub(crate) fn output(exit: i32, stdout: &str, stderr: &str) -> CommandOutput {
        CommandOutput {
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            exit_code: Some(exit),
            truncated: false,
        }
    }

    #[async_trait]
    impl SshTransport for ProbeTransport {
        async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
            self.log.lock().unwrap().push(format!("exec:{command}"));
            if command.contains("sudo -n") && command.contains(" -l ") {
                Ok(self.sudo_check.clone())
            } else {
                Ok(self.probe.clone())
            }
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
            unreachable!()
        }
        async fn open_sftp_via_exec(
            &mut self,
            command: &str,
        ) -> Result<Box<dyn SftpSession>, SshError> {
            self.log
                .lock()
                .unwrap()
                .push(format!("sftp-exec:{command}"));
            if let Some(reached) = &self.open_reached {
                reached.notify_one();
            }
            if let Some(gate) = &self.open_gate {
                gate.notified().await;
            }
            if self.start_fails {
                Err(SshError::ChannelError(
                    "SFTP-Init fehlgeschlagen".to_string(),
                ))
            } else {
                match self.prepared_channels.lock().unwrap().pop_front() {
                    Some(channel) => Ok(channel),
                    None => Ok(Box::new(MockSftpSession::new())),
                }
            }
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    pub(crate) fn transport(
        probe: CommandOutput,
        sudo_check: CommandOutput,
    ) -> (ProbeTransport, Arc<StdMutex<Vec<String>>>) {
        let log = Arc::new(StdMutex::new(Vec::new()));
        (
            ProbeTransport {
                probe,
                sudo_check,
                start_fails: false,
                log: log.clone(),
                open_reached: None,
                open_gate: None,
                prepared_channels: Arc::new(StdMutex::new(std::collections::VecDeque::new())),
            },
            log,
        )
    }

    /// Eine geglückte Probe plus geglückte sudo-Prüfung.
    pub(crate) fn working_probes() -> (CommandOutput, CommandOutput) {
        (
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(0, PATH, ""),
        )
    }

    /// Ein Transport, der jede Aktivierung gelingen lässt.
    pub(crate) fn working_transport() -> ProbeTransport {
        let (probe, check) = working_probes();
        transport(probe, check).0
    }

    /// Sitzung + `SessionManager` + Zuordnung, wie die Tauri-Befehle sie zur
    /// Laufzeit vorfinden.
    pub(crate) struct ElevationFixture {
        pub(crate) sessions: SessionManager,
        pub(crate) registry: ElevatedSftpRegistry,
        pub(crate) session_id: SessionId,
        pub(crate) session: Arc<Session>,
    }

    pub(crate) fn fixture(transport: Box<dyn SshTransport>) -> ElevationFixture {
        let sessions = SessionManager::new();
        // Spec 0084, A5: `session_with_transport` baut den `Session`-
        // Struct-Literal drüben in `app_logic::test_support` — der eine
        // kanonische Bauplan, statt ihn hier ein zweites Mal zu pflegen.
        let session = Arc::new(app_logic::test_support::session_with_transport(transport));
        let session_id = SessionId::new_v4();
        sessions.insert(session_id, session.clone());
        ElevationFixture {
            sessions,
            registry: ElevatedSftpRegistry::default(),
            session_id,
            session,
        }
    }

    /// Spec 0085, T4: wie [`fixture`], aber jede Aktivierung gelingt und gibt
    /// der Reihe nach die übergebenen Kanäle heraus.
    pub(crate) fn fixture_with_channels(channels: Vec<Box<dyn SftpSession>>) -> ElevationFixture {
        let mut transport = working_transport();
        transport.prepared_channels = Arc::new(StdMutex::new(channels.into()));
        fixture(Box::new(transport))
    }

    impl ElevationFixture {
        pub(crate) fn ctx(&self) -> ElevationContext<'_> {
            ElevationContext {
                sessions: &self.sessions,
                registry: &self.registry,
                session_id: self.session_id,
                session: &self.session,
            }
        }

        /// Ist für diese Sitzung ein erhöhter Kanal eingetragen und belegt?
        pub(crate) async fn is_active(&self) -> bool {
            match self.registry.slot(self.session_id, &access()) {
                Some(slot) => slot.lock_owned(&access()).await.is_some(),
                None => false,
            }
        }
    }
}
