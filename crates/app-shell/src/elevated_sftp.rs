//! Spec 0067, Teil A: der erhöhte SFTP-Kanal einer Session (`sudo -n
//! <sftp-server>` über einen Exec-Kanal). Getrennt vom normalen
//! `Session::sftp`, das auch KI-Aktionen (`read_remote_file`/
//! `write_remote_file`) nutzen.
//!
//! **Nur Browser-Commands kommen heran:** [`ElevatedSftpSlot::lock_owned`]
//! und jeder Zugriff auf die [`ElevatedSftpRegistry`] verlangen ein
//! [`crate::commands::BrowserAccess`], das nur im Modul `commands` erzeugt
//! werden kann. KI (`orchestration`) und MCP (`mcp_backend`) können den
//! Kanal damit nicht einmal ansprechen — ein Versuch scheitert schon beim
//! Kompilieren.
//!
//! **Spec 0084, A1:** Der Kanal hängt seit diesem Schritt nicht mehr am
//! `Session`-Wert, sondern an der [`ElevatedSftpRegistry`] — einem eigenen,
//! von Tauri verwalteten Zustand von `app-shell` (s. `crate::lib::run`).
//! Damit bleibt er in `app-shell`, wenn die übrige Anwendungslogik (und mit
//! ihr `Session`) in einen Tauri-freien Crate zieht.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use ssh_manager_core::ssh::SftpSession;

use crate::commands::BrowserAccess;
use crate::session::{Session, SessionManager};
use crate::state::SessionId;

pub struct ElevatedSftp {
    /// Ziel-Nutzer der Rechteerhöhung (Default `root`).
    pub target_user: String,
    pub sftp: Box<dyn SftpSession>,
}

/// Der erhöhte Kanal **einer** Sitzung mit seiner eigenen Sperre.
///
/// Nie persistiert: lebt nur in der [`ElevatedSftpRegistry`] und verschwindet
/// mit dem Eintrag dort (Verbindungstrennung, App-Neustart).
///
/// Spec 0084, §4: Die Sperre der Zuordnung (`ElevatedSftpRegistry`) gilt nur
/// für Nachschlagen, Eintragen und Entfernen; der Kanal selbst behält diese
/// eigene Sperre je Sitzung, damit ein laufender Vorgang in einer Sitzung das
/// Trennen einer anderen nicht aufhält. Der `Arc` macht die Sperre
/// unabhängig von der Karte haltbar: ein Browser-Befehl, der den Kanal
/// gerade nutzt, hält seinen eigenen Zeiger, während die Zuordnung schon
/// wieder frei ist.
#[derive(Clone, Default)]
pub struct ElevatedSftpSlot(Arc<AsyncMutex<Option<ElevatedSftp>>>);

impl ElevatedSftpSlot {
    fn with_channel(channel: ElevatedSftp) -> Self {
        Self(Arc::new(AsyncMutex::new(Some(channel))))
    }

    pub(crate) async fn lock_owned(
        &self,
        _access: &BrowserAccess,
    ) -> OwnedMutexGuard<Option<ElevatedSftp>> {
        self.0.clone().lock_owned().await
    }
}

/// Spec 0084, A1/A2: Zuordnung Sitzungskennung → erhöhter Kanal.
///
/// Eigener, von Tauri verwalteter Zustand von `app-shell` (kein Feld von
/// `AppState`), damit der erhöhte Kanal auch dann in `app-shell` bleibt,
/// wenn `AppState`/`Session` in den Tauri-freien Logik-Crate ziehen.
///
/// **Sperrdisziplin (A2).** Die `slots`-Sperre ist die eine Sperre, unter der
/// ein Eintrag entsteht ([`Self::insert_if_session_alive`]) und verschwindet
/// ([`Self::remove`], [`Self::remove_session`]). Das Eintragen prüft unter
/// dieser Sperre, dass die Sitzung noch im [`SessionManager`] steht; das
/// Entfernen einer Sitzung nimmt sie **nach** `SessionManager::remove`. Aus
/// beidem folgt, dass kein Eintrag seine Sitzung überleben kann:
/// * Eintrag vor `SessionManager::remove` → [`Self::remove_session`] löscht ihn.
/// * Eintrag nach `SessionManager::remove` → die Sitzung ist weg, die Prüfung
///   schlägt fehl, der Kanal wird verworfen.
///
/// Die umgekehrte Reihenfolge in [`Self::remove_session`] (erst Eintrag, dann
/// Sitzung) wäre genau die Lücke, in der ein gleichzeitiges Aktivieren einen
/// Eintrag für eine gleich darauf entfernte Sitzung anlegt — Regressionstest
/// `test_t8b_activation_between_the_two_removal_steps_leaves_no_entry`.
#[derive(Default)]
pub struct ElevatedSftpRegistry {
    slots: StdMutex<HashMap<SessionId, ElevatedSftpSlot>>,
    /// Nur für den Regressionstest T8b (Spec 0084, §6): ein Haltepunkt
    /// **zwischen** den beiden Schritten von [`Self::remove_session`]. Der
    /// Test lässt dort ein gleichzeitiges Aktivieren vollständig
    /// durchlaufen; ohne diesen Haltepunkt wäre die Verschränkung nur
    /// zufällig zu treffen. Existiert in keinem Produktivbau.
    #[cfg(test)]
    interleave_hook: StdMutex<Option<InterleaveHook>>,
}

/// s. [`ElevatedSftpRegistry::interleave_hook`].
#[cfg(test)]
type InterleaveHook = Box<dyn Fn() + Send>;

impl ElevatedSftpRegistry {
    /// Der erhöhte Kanal dieser Sitzung, sofern aktiv.
    pub(crate) fn slot(
        &self,
        session_id: SessionId,
        _access: &BrowserAccess,
    ) -> Option<ElevatedSftpSlot> {
        self.slots.lock().unwrap().get(&session_id).cloned()
    }

    /// Spec 0084, A2.2: trägt `channel` **nur** ein, wenn `session_id` zu
    /// diesem Zeitpunkt noch im `SessionManager` steht. Prüfung und Eintrag
    /// geschehen unter der `slots`-Sperre — derselben, unter der
    /// [`Self::remove_session`] den Eintrag löscht.
    ///
    /// `false` heißt: nichts eingetragen, `channel` wird hier verworfen (sein
    /// `Drop` schließt den Exec-Kanal).
    fn insert_if_session_alive(
        &self,
        sessions: &SessionManager,
        session_id: SessionId,
        channel: ElevatedSftp,
        _access: &BrowserAccess,
    ) -> bool {
        let mut slots = self.slots.lock().unwrap();
        if sessions.get(session_id).is_none() {
            return false;
        }
        slots.insert(session_id, ElevatedSftpSlot::with_channel(channel));
        true
    }

    /// Entfernt den Kanal einer Sitzung; `true`, wenn einer aktiv war.
    fn remove(&self, session_id: SessionId, _access: &BrowserAccess) -> bool {
        self.slots.lock().unwrap().remove(&session_id).is_some()
    }

    /// Spec 0084, A2.1: **die eine** Funktion, die eine Sitzung samt ihrem
    /// erhöhten Kanal entfernt. Außer ihr und Tests ruft nichts
    /// `SessionManager::remove` auf.
    ///
    /// Reihenfolge ist Teil der Zusage: `SessionManager::remove` läuft
    /// **vor** dem Löschen des Zuordnungs-Eintrags (s. Sperrdisziplin am
    /// Typ). Der Transport der Sitzung wird hier nicht gesperrt — das
    /// Trennen bleibt danach in `commands::disconnect`.
    pub(crate) fn remove_session(
        &self,
        sessions: &SessionManager,
        session_id: SessionId,
    ) -> Option<Arc<Session>> {
        let session = sessions.remove(session_id);
        #[cfg(test)]
        {
            let hook = self.interleave_hook.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        self.slots.lock().unwrap().remove(&session_id);
        session
    }

    /// Nur für Tests, die einen aktiven Kanal als Ausgangslage brauchen,
    /// ohne den vollen Aktivierungs-Ablauf (Proben, sudo-Prüfung) zu fahren
    /// — die Prüfung aus A2.2 gehört zu genau diesem Ablauf und wird
    /// separat geprüft (T8/T8b).
    #[cfg(test)]
    pub(crate) fn insert_for_tests(&self, session_id: SessionId, channel: ElevatedSftp) {
        self.slots
            .lock()
            .unwrap()
            .insert(session_id, ElevatedSftpSlot::with_channel(channel));
    }

    /// Nur für T8b — s. [`Self::interleave_hook`]. Der Haltepunkt wird beim
    /// ersten Durchlauf verbraucht.
    #[cfg(test)]
    fn set_interleave_hook(&self, hook: InterleaveHook) {
        *self.interleave_hook.lock().unwrap() = Some(hook);
    }
}

use ssh_manager_core::ssh::elevated::{
    classify_sudo_check, elevated_sftp_command, is_plausible_sftp_server_path,
    is_valid_target_user, parse_sftp_server_probe, sftp_server_probe_command, sudo_check_command,
    sudoers_line, SudoCheck, DEFAULT_ELEVATION_USER,
};

use crate::dto::{ElevationFailureDto, ElevationFailureKind, ElevationResultDto};
use crate::error::{CommandError, CommandResult};

/// Spec 0084, A1/A2: alles, was das Einschalten des erhöhten Kanals über die
/// Sitzungsgrenze hinweg braucht — gebündelt, weil `enable` sonst über die
/// Argumentgrenze liefe.
pub(crate) struct ElevationContext<'a> {
    pub sessions: &'a SessionManager,
    pub registry: &'a ElevatedSftpRegistry,
    pub session_id: SessionId,
    pub session: &'a Session,
}

/// Obergrenze je Probe-Kommando (Pfad-Erkennung, `sudo -n -l`) — `-n` lässt
/// sudo nie auf ein Passwort warten, das hier ist nur das Sicherheitsnetz.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Liefert bewusst `Ok`: ein gescheiterter Aktivierungsversuch ist ein
/// strukturiertes Ergebnis für die Oberfläche (Spec 0067, A1–A3), kein
/// Befehlsfehler. `Err` bleibt dem einen Fall aus Spec 0084, A2.2 vorbehalten
/// (Sitzung während des Aktivierens verschwunden).
fn failure(
    target_user: &str,
    sftp_server_path: Option<String>,
    kind: ElevationFailureKind,
    sudoers: Option<String>,
    detail: Option<String>,
) -> CommandResult<ElevationResultDto> {
    Ok(ElevationResultDto {
        active: false,
        target_user: target_user.to_string(),
        sftp_server_path,
        failure: Some(ElevationFailureDto {
            kind,
            sudoers_line: sudoers,
            detail,
        }),
    })
}

async fn run_probe(
    session: &Session,
    command: &str,
) -> Result<ssh_manager_core::ssh::CommandOutput, String> {
    let mut transport = session.transport.lock().await;
    match tokio::time::timeout(PROBE_TIMEOUT, transport.execute(command)).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err("Zeitüberschreitung".to_string()),
    }
}

/// Spec 0067, A1–A3: schaltet den erhöhten Kanal für `ctx.session` ein. Nie
/// ein Passwort (`sudo -n`), nie hängen (Timeouts), bei jedem Fehler eine
/// strukturierte Meldung statt eines kryptischen Fehlers. Die Probe-Ausgaben
/// (inkl. sudo-stderr) gehen nur in diese Meldung, nie in Chat/KI-Kontext.
///
/// Spec 0084, A2.2: Der geöffnete Kanal wird nur eingetragen, solange die
/// Sitzung noch steht — sonst `Err("Session nicht gefunden")` und der Kanal
/// wird verworfen.
pub(crate) async fn enable(
    ctx: &ElevationContext<'_>,
    login: &str,
    override_path: Option<&str>,
    requested_user: Option<&str>,
    access: &BrowserAccess,
) -> CommandResult<ElevationResultDto> {
    let session = ctx.session;
    let target_user = requested_user
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .unwrap_or(DEFAULT_ELEVATION_USER)
        .to_string();
    if !is_valid_target_user(&target_user) {
        return failure(
            &target_user,
            None,
            ElevationFailureKind::InvalidUser,
            None,
            None,
        );
    }

    let path = match override_path {
        Some(path) if is_plausible_sftp_server_path(path) => path.to_string(),
        Some(_) => {
            return failure(
                &target_user,
                None,
                ElevationFailureKind::InvalidPath,
                None,
                None,
            );
        }
        None => match run_probe(session, &sftp_server_probe_command()).await {
            Ok(output) => match parse_sftp_server_probe(&output) {
                Some(path) => path,
                None => {
                    return failure(
                        &target_user,
                        None,
                        ElevationFailureKind::SftpServerNotFound,
                        None,
                        None,
                    );
                }
            },
            Err(detail) => {
                return failure(
                    &target_user,
                    None,
                    ElevationFailureKind::CheckFailed,
                    None,
                    Some(detail),
                );
            }
        },
    };
    let path_for_dto = Some(path.clone());

    let check_cmd =
        sudo_check_command(&path, &target_user).expect("Pfad und Nutzer wurden oben validiert");
    let check = match run_probe(session, &check_cmd).await {
        Ok(output) => classify_sudo_check(&output),
        Err(detail) => SudoCheck::Failed(detail),
    };
    let sudoers = || sudoers_line(login, &path, &target_user);
    match check {
        SudoCheck::Allowed => {}
        SudoCheck::PasswordRequired => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::PasswordRequired,
                sudoers(),
                None,
            );
        }
        SudoCheck::NotAllowed => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::NotAllowed,
                sudoers(),
                None,
            );
        }
        SudoCheck::RequireTty => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::RequireTty,
                None,
                None,
            );
        }
        SudoCheck::SudoMissing => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::SudoMissing,
                None,
                None,
            );
        }
        SudoCheck::Failed(detail) => {
            return failure(
                &target_user,
                path_for_dto,
                ElevationFailureKind::CheckFailed,
                None,
                Some(detail),
            );
        }
    }

    let command =
        elevated_sftp_command(&path, &target_user).expect("Pfad und Nutzer wurden oben validiert");
    let opened = {
        let mut transport = session.transport.lock().await;
        transport.open_sftp_via_exec(&command).await
    };
    match opened {
        Ok(sftp) => {
            // Spec 0084, A2.2: Zwischen den Proben oben und diesem Eintrag
            // liegen mehrere `.await`-Punkte — die Sitzung kann inzwischen
            // getrennt worden sein. Dann wird der eben geöffnete Kanal hier
            // verworfen (sein `Drop` schließt ihn), statt als Eintrag ohne
            // Sitzung stehen zu bleiben.
            let channel = ElevatedSftp {
                target_user: target_user.clone(),
                sftp,
            };
            if !ctx
                .registry
                .insert_if_session_alive(ctx.sessions, ctx.session_id, channel, access)
            {
                return Err(CommandError::from("Session nicht gefunden"));
            }
            tracing::info!(
                target_user = %target_user,
                sftp_server_path = %path,
                source = "manual",
                "file browser elevated rights enabled"
            );
            Ok(ElevationResultDto {
                active: true,
                target_user,
                sftp_server_path: path_for_dto,
                failure: None,
            })
        }
        Err(err) => failure(
            &target_user,
            path_for_dto,
            ElevationFailureKind::StartFailed,
            None,
            Some(err.to_string()),
        ),
    }
}

/// Schaltet den erhöhten Kanal einer Sitzung aus (verwirft ihn).
///
/// Spec 0084, T9: braucht die Sitzung selbst nicht — für eine unbekannte
/// Sitzungskennung und für eine Sitzung ohne aktiven Kanal ist das Ergebnis
/// dasselbe wie bisher bei „nicht aktiv“: nichts passiert.
pub(crate) fn disable(
    registry: &ElevatedSftpRegistry,
    session_id: SessionId,
    access: &BrowserAccess,
) {
    if registry.remove(session_id, access) {
        tracing::info!(source = "manual", "file browser elevated rights disabled");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use async_trait::async_trait;
    use ssh_manager_core::ssh::mock::MockSftpSession;
    use ssh_manager_core::ssh::{CommandOutput, InteractiveShell, PtySize, SshError, SshTransport};

    use super::*;
    use crate::commands::BrowserAccess;
    use crate::test_support::session_with_transport;

    /// Beantwortet die Probe-Kommandos nach Präfix und zeichnet alle
    /// Kommandos sowie Exec-SFTP-Starts auf.
    struct ProbeTransport {
        probe: CommandOutput,
        sudo_check: CommandOutput,
        start_fails: bool,
        log: Arc<StdMutex<Vec<String>>>,
        /// Spec 0084, T8/T8b: meldet, dass das Öffnen des erhöhten Kanals
        /// erreicht ist — also alle Proben durch sind und nur noch der
        /// Eintrag in die Zuordnung fehlt.
        open_reached: Option<Arc<tokio::sync::Notify>>,
        /// Spec 0084, T8/T8b: hält das Öffnen dort an, bis der Test
        /// freigibt.
        open_gate: Option<Arc<tokio::sync::Notify>>,
    }

    fn output(exit: i32, stdout: &str, stderr: &str) -> CommandOutput {
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
        ) -> Result<Box<dyn ssh_manager_core::ssh::SftpSession>, SshError> {
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
                Ok(Box::new(MockSftpSession::new()))
            }
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    fn transport(
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
            },
            log,
        )
    }

    const PATH: &str = "/usr/lib/openssh/sftp-server";

    fn access() -> BrowserAccess {
        BrowserAccess::for_tests()
    }

    /// Spec 0084, A1: Sitzung, `SessionManager` und Zuordnung gehören seit
    /// diesem Schritt zusammen — der erhöhte Kanal hängt nicht mehr am
    /// `Session`-Wert.
    struct Fixture {
        sessions: SessionManager,
        registry: ElevatedSftpRegistry,
        session_id: SessionId,
        session: Arc<Session>,
    }

    fn fixture(transport: Box<dyn SshTransport>) -> Fixture {
        let sessions = SessionManager::new();
        let session = Arc::new(session_with_transport(transport));
        let session_id = SessionId::new_v4();
        sessions.insert(session_id, session.clone());
        Fixture {
            sessions,
            registry: ElevatedSftpRegistry::default(),
            session_id,
            session,
        }
    }

    impl Fixture {
        fn ctx(&self) -> ElevationContext<'_> {
            ElevationContext {
                sessions: &self.sessions,
                registry: &self.registry,
                session_id: self.session_id,
                session: &self.session,
            }
        }

        /// Ist für diese Sitzung ein erhöhter Kanal eingetragen und belegt?
        async fn is_active(&self) -> bool {
            match self.registry.slot(self.session_id, &access()) {
                Some(slot) => slot.lock_owned(&access()).await.is_some(),
                None => false,
            }
        }
    }

    /// Eine geglückte Probe plus geglückte sudo-Prüfung.
    fn working_probes() -> (CommandOutput, CommandOutput) {
        (
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(0, PATH, ""),
        )
    }

    #[tokio::test]
    async fn test_enable_probes_path_checks_sudo_and_opens_exec_channel() {
        let (probe, check) = working_probes();
        let (t, log) = transport(probe, check);
        let f = fixture(Box::new(t));

        let result = enable(&f.ctx(), "deploy", None, None, &access())
            .await
            .expect("Sitzung steht, das Aktivieren darf nicht mit Err enden");

        assert!(result.active, "{result:?}");
        assert_eq!(result.target_user, "root");
        assert_eq!(result.sftp_server_path.as_deref(), Some(PATH));
        let log = log.lock().unwrap().clone();
        assert_eq!(log.last().unwrap(), &format!("sftp-exec:sudo -n {PATH}"));
        assert!(log
            .iter()
            .any(|c| c == &format!("exec:env LC_ALL=C sudo -n -l {PATH}")));
        assert!(f.is_active().await);
    }

    #[tokio::test]
    async fn test_password_required_yields_tailored_sudoers_line_and_no_channel() {
        let (t, log) = transport(
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(1, "", "sudo: a password is required\n"),
        );
        let f = fixture(Box::new(t));

        let result = enable(&f.ctx(), "deploy", None, None, &access())
            .await
            .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");

        assert!(!result.active);
        let failure = result.failure.unwrap();
        assert_eq!(failure.kind, ElevationFailureKind::PasswordRequired);
        assert_eq!(
            failure.sudoers_line.as_deref(),
            Some("deploy ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server \"\"")
        );
        assert!(!log
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("sftp-exec:")));
        assert!(!f.is_active().await);
    }

    #[tokio::test]
    async fn test_each_prerequisite_failure_is_named() {
        for (probe, check, expected) in [
            (
                output(3, "", ""),
                output(0, "", ""),
                ElevationFailureKind::SftpServerNotFound,
            ),
            (
                output(0, "/usr/lib/openssh/sftp-server\n", ""),
                output(1, "", "sudo: sorry, you must have a tty to run sudo\n"),
                ElevationFailureKind::RequireTty,
            ),
            (
                output(0, "/usr/lib/openssh/sftp-server\n", ""),
                output(127, "", "sh: 1: sudo: not found\n"),
                ElevationFailureKind::SudoMissing,
            ),
        ] {
            let (t, _log) = transport(probe, check);
            let f = fixture(Box::new(t));
            let result = enable(&f.ctx(), "deploy", None, None, &access())
                .await
                .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");
            assert_eq!(result.failure.map(|f| f.kind), Some(expected));
        }
    }

    #[tokio::test]
    async fn test_override_path_skips_probe_and_invalid_override_runs_nothing() {
        let (t, log) = transport(output(3, "", ""), output(0, "", ""));
        let f = fixture(Box::new(t));
        let result = enable(
            &f.ctx(),
            "deploy",
            Some("/opt/ssh/sftp-server"),
            Some("www-data"),
            &access(),
        )
        .await
        .expect("Sitzung steht, das Aktivieren darf nicht mit Err enden");
        assert!(result.active, "{result:?}");
        let log = log.lock().unwrap().clone();
        assert!(
            !log.iter().any(|c| c.contains("sshd_config")),
            "keine Probe bei Override"
        );
        assert_eq!(
            log.last().unwrap(),
            "sftp-exec:sudo -n -u www-data /opt/ssh/sftp-server"
        );

        let (t, log) = transport(output(0, "", ""), output(0, "", ""));
        let f = fixture(Box::new(t));
        let result = enable(&f.ctx(), "deploy", Some("/opt/x; reboot"), None, &access())
            .await
            .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");
        assert_eq!(
            result.failure.map(|f| f.kind),
            Some(ElevationFailureKind::InvalidPath)
        );
        assert!(
            log.lock().unwrap().is_empty(),
            "nichts darf ausgeführt werden"
        );
    }

    #[tokio::test]
    async fn test_start_failure_is_reported_and_leaves_no_channel() {
        let (probe, check) = working_probes();
        let (mut t, _log) = transport(probe, check);
        t.start_fails = true;
        let f = fixture(Box::new(t));

        let result = enable(&f.ctx(), "deploy", None, None, &access())
            .await
            .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");

        assert_eq!(
            result.failure.map(|f| f.kind),
            Some(ElevationFailureKind::StartFailed)
        );
        assert!(!f.is_active().await);
    }

    // --- Spec 0084, A2: kein erhöhter Kanal überlebt seine Sitzung ---------

    /// Spec 0084, T6: Ist ein Kanal aktiv und wird die Sitzung über die eine
    /// Funktion aus A2.1 entfernt, bleibt kein Eintrag in der Zuordnung
    /// zurück. Scheitert, wenn diese Funktion den Eintrag nicht löscht.
    #[tokio::test]
    async fn test_t6_removing_a_session_also_removes_its_elevated_channel() {
        let (probe, check) = working_probes();
        let (t, _log) = transport(probe, check);
        let f = fixture(Box::new(t));
        enable(&f.ctx(), "deploy", None, None, &access())
            .await
            .expect("Vorbedingung: Aktivieren gelingt");
        assert!(f.is_active().await, "Vorbedingung: Kanal ist aktiv");

        let removed = f.registry.remove_session(&f.sessions, f.session_id);

        assert!(removed.is_some(), "die Sitzung selbst muss entfernt sein");
        assert!(
            f.registry.slot(f.session_id, &access()).is_none(),
            "A2.1: der erhöhte Kanal darf seine Sitzung nicht überleben"
        );
    }

    /// Spec 0084, T8: Das Aktivieren steht nach den Proben, unmittelbar vor
    /// dem Eintrag, still; in diesem Moment wird die Sitzung über A2.1
    /// entfernt. Danach läuft das Aktivieren weiter und muss mit dem
    /// bestehenden Fehler enden, ohne einen Eintrag zu hinterlassen.
    /// Scheitert, wenn die Prüfung aus A2.2 fehlt.
    #[tokio::test]
    async fn test_t8_activation_finishing_after_the_session_was_removed_is_rejected() {
        let reached = Arc::new(tokio::sync::Notify::new());
        let gate = Arc::new(tokio::sync::Notify::new());
        let (probe, check) = working_probes();
        let (mut t, _log) = transport(probe, check);
        t.open_reached = Some(reached.clone());
        t.open_gate = Some(gate.clone());
        let f = Arc::new(fixture(Box::new(t)));

        let f_for_enable = f.clone();
        let activation = tokio::spawn(async move {
            enable(&f_for_enable.ctx(), "deploy", None, None, &access()).await
        });

        // Warten, bis das Aktivieren wirklich vor dem Eintrag steht — sonst
        // prüfte der Test eine andere Verschränkung als gemeint.
        tokio::time::timeout(std::time::Duration::from_secs(5), reached.notified())
            .await
            .expect("das Aktivieren muss den Öffnen-Schritt erreichen");

        f.registry.remove_session(&f.sessions, f.session_id);
        gate.notify_one();

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), activation)
            .await
            .expect("das Aktivieren muss enden")
            .expect("der Aktivierungs-Task darf nicht panisch enden");

        let err = result.expect_err("A2.2: ohne Sitzung darf nicht aktiviert werden");
        assert_eq!(err.message, "Session nicht gefunden");
        assert!(
            f.registry.slot(f.session_id, &access()).is_none(),
            "A2.2: kein Eintrag für eine bereits entfernte Sitzung"
        );
    }

    /// Spec 0084, T8b: dieselbe Verschränkung, aber das Aktivieren läuft
    /// vollständig durch, während A2.1 zwischen `SessionManager::remove` und
    /// dem Löschen des Zuordnungs-Eintrags steht (Haltepunkt
    /// `interleave_hook`). Scheitert bei falscher Reihenfolge in A2.1:
    /// löschte A2.1 erst den Eintrag und entfernte danach die Sitzung, sähe
    /// das Aktivieren die Sitzung noch, trüge den Kanal ein — und der
    /// Eintrag überlebte die Sitzung.
    #[tokio::test]
    async fn test_t8b_activation_between_the_two_removal_steps_leaves_no_entry() {
        let reached = Arc::new(tokio::sync::Notify::new());
        let gate = Arc::new(tokio::sync::Notify::new());
        let (probe, check) = working_probes();
        let (mut t, _log) = transport(probe, check);
        t.open_reached = Some(reached.clone());
        t.open_gate = Some(gate.clone());
        let f = Arc::new(fixture(Box::new(t)));

        let f_for_enable = f.clone();
        let activation = tokio::spawn(async move {
            enable(&f_for_enable.ctx(), "deploy", None, None, &access()).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), reached.notified())
            .await
            .expect("das Aktivieren muss den Öffnen-Schritt erreichen");

        // Der Haltepunkt läuft mitten in A2.1: er gibt das Aktivieren frei
        // und wartet, bis es ganz durch ist, bevor A2.1 seinen zweiten
        // Schritt macht.
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let gate_for_hook = gate.clone();
        f.registry.set_interleave_hook(Box::new(move || {
            gate_for_hook.notify_one();
            done_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("das Aktivieren muss innerhalb des Haltepunkts enden");
        }));

        let f_for_removal = f.clone();
        let removal = tokio::task::spawn_blocking(move || {
            f_for_removal
                .registry
                .remove_session(&f_for_removal.sessions, f_for_removal.session_id)
        });

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), activation)
            .await
            .expect("das Aktivieren muss enden")
            .expect("der Aktivierungs-Task darf nicht panisch enden");
        let _ = done_tx.send(());
        tokio::time::timeout(std::time::Duration::from_secs(5), removal)
            .await
            .expect("A2.1 muss enden")
            .expect("der Entfernen-Task darf nicht panisch enden");

        assert!(
            result.is_err(),
            "A2.1 entfernt die Sitzung zuerst — danach darf nicht mehr aktiviert werden"
        );
        assert!(
            f.registry.slot(f.session_id, &access()).is_none(),
            "A2.1/A2.2: kein Eintrag darf die Sitzung überleben"
        );
    }

    /// Spec 0084, T9: Ausschalten ohne aktiven Kanal und für eine unbekannte
    /// Sitzungskennung — kein Absturz, Ergebnis wie bisher bei „nicht aktiv“.
    #[tokio::test]
    async fn test_t9_disabling_an_inactive_or_unknown_session_channel_is_harmless() {
        let (probe, check) = working_probes();
        let (t, _log) = transport(probe, check);
        let f = fixture(Box::new(t));

        disable(&f.registry, f.session_id, &access());
        assert!(!f.is_active().await);

        disable(&f.registry, SessionId::new_v4(), &access());
        assert!(!f.is_active().await);
    }
}
