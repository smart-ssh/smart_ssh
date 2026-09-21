//! Spec 0067, Teil A: der erhöhte SFTP-Kanal einer Session (`sudo -n
//! <sftp-server>` über einen Exec-Kanal). Getrennt vom normalen
//! `Session::sftp`, das auch KI-Aktionen (`read_remote_file`/
//! `write_remote_file`) nutzen.
//!
//! **Nur Browser-Commands kommen heran:** [`ElevatedSftpSlot::lock`]
//! verlangt ein [`crate::commands::BrowserAccess`], das nur im Modul
//! `commands` erzeugt werden kann. KI (`orchestration`) und MCP
//! (`mcp_backend`) können den Kanal damit nicht einmal ansprechen — ein
//! Versuch scheitert schon beim Kompilieren.

use tokio::sync::{Mutex as AsyncMutex, MutexGuard};

use ssh_manager_core::ssh::SftpSession;

use crate::commands::BrowserAccess;

pub struct ElevatedSftp {
    /// Ziel-Nutzer der Rechteerhöhung (Default `root`).
    pub target_user: String,
    pub sftp: Box<dyn SftpSession>,
}

/// Nie persistiert: lebt nur in der `Session` und verschwindet mit ihr
/// (Verbindungstrennung, App-Neustart).
#[derive(Default)]
pub struct ElevatedSftpSlot(AsyncMutex<Option<ElevatedSftp>>);

impl ElevatedSftpSlot {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn lock(
        &self,
        _access: &BrowserAccess,
    ) -> MutexGuard<'_, Option<ElevatedSftp>> {
        self.0.lock().await
    }
}

use ssh_manager_core::ssh::elevated::{
    classify_sudo_check, elevated_sftp_command, is_plausible_sftp_server_path,
    is_valid_target_user, parse_sftp_server_probe, sftp_server_probe_command, sudo_check_command,
    sudoers_line, SudoCheck, DEFAULT_ELEVATION_USER,
};

use crate::dto::{ElevationFailureDto, ElevationFailureKind, ElevationResultDto};
use crate::session::Session;

/// Obergrenze je Probe-Kommando (Pfad-Erkennung, `sudo -n -l`) — `-n` lässt
/// sudo nie auf ein Passwort warten, das hier ist nur das Sicherheitsnetz.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

fn failure(
    target_user: &str,
    sftp_server_path: Option<String>,
    kind: ElevationFailureKind,
    sudoers: Option<String>,
    detail: Option<String>,
) -> ElevationResultDto {
    ElevationResultDto {
        active: false,
        target_user: target_user.to_string(),
        sftp_server_path,
        failure: Some(ElevationFailureDto {
            kind,
            sudoers_line: sudoers,
            detail,
        }),
    }
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

/// Spec 0067, A1–A3: schaltet den erhöhten Kanal für `session` ein. Nie ein
/// Passwort (`sudo -n`), nie hängen (Timeouts), bei jedem Fehler eine
/// strukturierte Meldung statt eines kryptischen Fehlers. Die Probe-Ausgaben
/// (inkl. sudo-stderr) gehen nur in diese Meldung, nie in Chat/KI-Kontext.
pub(crate) async fn enable(
    session: &Session,
    login: &str,
    override_path: Option<&str>,
    requested_user: Option<&str>,
    access: &BrowserAccess,
) -> ElevationResultDto {
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
            tracing::info!(
                target_user = %target_user,
                sftp_server_path = %path,
                source = "manual",
                "file browser elevated rights enabled"
            );
            *session.elevated_sftp.lock(access).await = Some(ElevatedSftp {
                target_user: target_user.clone(),
                sftp,
            });
            ElevationResultDto {
                active: true,
                target_user,
                sftp_server_path: path_for_dto,
                failure: None,
            }
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

/// Schaltet den erhöhten Kanal aus (verwirft ihn).
pub(crate) async fn disable(session: &Session, access: &BrowserAccess) {
    if session.elevated_sftp.lock(access).await.take().is_some() {
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
            },
            log,
        )
    }

    const PATH: &str = "/usr/lib/openssh/sftp-server";

    #[tokio::test]
    async fn test_enable_probes_path_checks_sudo_and_opens_exec_channel() {
        let (t, log) = transport(
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(0, PATH, ""),
        );
        let session = session_with_transport(Box::new(t));

        let result = enable(&session, "deploy", None, None, &BrowserAccess::for_tests()).await;

        assert!(result.active, "{result:?}");
        assert_eq!(result.target_user, "root");
        assert_eq!(result.sftp_server_path.as_deref(), Some(PATH));
        let log = log.lock().unwrap().clone();
        assert_eq!(log.last().unwrap(), &format!("sftp-exec:sudo -n {PATH}"));
        assert!(log
            .iter()
            .any(|c| c == &format!("exec:env LC_ALL=C sudo -n -l {PATH}")));
        assert!(session
            .elevated_sftp
            .lock(&BrowserAccess::for_tests())
            .await
            .is_some());
    }

    #[tokio::test]
    async fn test_password_required_yields_tailored_sudoers_line_and_no_channel() {
        let (t, log) = transport(
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(1, "", "sudo: a password is required\n"),
        );
        let session = session_with_transport(Box::new(t));

        let result = enable(&session, "deploy", None, None, &BrowserAccess::for_tests()).await;

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
        assert!(session
            .elevated_sftp
            .lock(&BrowserAccess::for_tests())
            .await
            .is_none());
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
            let session = session_with_transport(Box::new(t));
            let result = enable(&session, "deploy", None, None, &BrowserAccess::for_tests()).await;
            assert_eq!(result.failure.map(|f| f.kind), Some(expected));
        }
    }

    #[tokio::test]
    async fn test_override_path_skips_probe_and_invalid_override_runs_nothing() {
        let (t, log) = transport(output(3, "", ""), output(0, "", ""));
        let session = session_with_transport(Box::new(t));
        let result = enable(
            &session,
            "deploy",
            Some("/opt/ssh/sftp-server"),
            Some("www-data"),
            &BrowserAccess::for_tests(),
        )
        .await;
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
        let session = session_with_transport(Box::new(t));
        let result = enable(
            &session,
            "deploy",
            Some("/opt/x; reboot"),
            None,
            &BrowserAccess::for_tests(),
        )
        .await;
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
        let (mut t, _log) = transport(
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(0, PATH, ""),
        );
        t.start_fails = true;
        let session = session_with_transport(Box::new(t));

        let result = enable(&session, "deploy", None, None, &BrowserAccess::for_tests()).await;

        assert_eq!(
            result.failure.map(|f| f.kind),
            Some(ElevationFailureKind::StartFailed)
        );
        assert!(session
            .elevated_sftp
            .lock(&BrowserAccess::for_tests())
            .await
            .is_none());
    }
}
