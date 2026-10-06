//! Spec 0102: Startverzeichnis einer Sitzung für das interaktive Terminal
//! und den SFTP-Dateibrowser.
//!
//! Der konfigurierte Wert ([`ssh_manager_core::profiles::Server::
//! start_directory`]) wird **einmal pro Sitzung** per SFTP `stat` geprüft;
//! Terminal und Dateibrowser teilen dieses eine Ergebnis und genau einen
//! Hinweis, falls das Verzeichnis fehlt. Fehlt es (oder ist es nicht
//! erreichbar), starten beide wie bisher im Home — die Sitzung selbst
//! scheitert daran nie.
//!
//! **Kein Teil des KI-Ausführungspfads.** `orchestration`/`action_exec`,
//! Filter-Engine und Risiko-Klassifizierer lesen diesen Zustand nicht; ein
//! KI-Kommando läuft unabhängig vom Startverzeichnis immer im Home
//! (ADR 0059).

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tokio::sync::OnceCell;

use ssh_manager_core::profiles::{start_directory_cd_command, start_directory_sftp_path};
use ssh_manager_core::ssh::{InteractiveShell, PtySize, SshError};

use crate::session::Session;

/// Ergebnis der einmaligen Prüfung des Startverzeichnisses einer Sitzung.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartDirectoryResolution {
    /// Kein Startverzeichnis konfiguriert — alles wie bisher.
    NotSet,
    /// Konfiguriert und als Verzeichnis erreichbar.
    Found { configured: String },
    /// Konfiguriert, aber nicht vorhanden, kein Verzeichnis oder nicht
    /// erreichbar (auch: SFTP ließ sich nicht öffnen). Terminal und
    /// Dateibrowser nutzen das Home.
    Missing { configured: String },
}

/// Zustand je Sitzung — liegt als privates Feld an [`Session`].
#[derive(Debug, Default)]
pub(crate) struct SessionStartDirectory {
    configured: Option<String>,
    resolution: OnceCell<StartDirectoryResolution>,
    notice_taken: AtomicBool,
}

impl SessionStartDirectory {
    pub(crate) fn new(configured: Option<String>) -> Self {
        Self {
            configured,
            ..Self::default()
        }
    }
}

/// Was der Dateibrowser beim Öffnen braucht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDirectoryDto {
    /// SFTP-Pfad, unter dem der Browser startet und zu dem „Zum
    /// Startverzeichnis" zurückkehrt — `"."` (Home), wenn nichts
    /// konfiguriert oder das Verzeichnis nicht da ist.
    pub path: String,
    /// `Some(konfigurierter Wert)` genau beim ersten Abruf des Hinweises in
    /// dieser Sitzung, wenn das Verzeichnis fehlt — s. [`take_missing_notice`].
    pub missing_directory: Option<String>,
}

/// Prüft das Startverzeichnis dieser Sitzung (höchstens einmal) und gibt das
/// gemeinsame Ergebnis zurück. Parallele Aufrufer warten auf dieselbe
/// Prüfung.
pub async fn resolve(session: &Session) -> &StartDirectoryResolution {
    let state = session.start_directory_state();
    state
        .resolution
        .get_or_init(|| async {
            let Some(configured) = state.configured.clone() else {
                return StartDirectoryResolution::NotSet;
            };
            if directory_exists(session, &start_directory_sftp_path(&configured)).await {
                StartDirectoryResolution::Found { configured }
            } else {
                tracing::info!(
                    server_id = %session.server_id.0,
                    "configured start directory not found, falling back to home",
                );
                StartDirectoryResolution::Missing { configured }
            }
        })
        .await
}

async fn directory_exists(session: &Session, sftp_path: &str) -> bool {
    if crate::orchestration::ensure_sftp_open(session)
        .await
        .is_err()
    {
        return false;
    }
    let mut guard = session.lock_sftp().await;
    let Some(sftp) = guard.sftp() else {
        return false;
    };
    matches!(sftp.stat(sftp_path).await, Ok(entry) if entry.is_dir)
}

/// Gibt den konfigurierten Wert genau **einmal** pro Sitzung zurück, wenn
/// das Startverzeichnis fehlt — damit Terminal und Dateibrowser zusammen
/// nur einen Hinweis zeigen, egal wer zuerst fragt.
fn take_missing_notice(session: &Session, resolution: &StartDirectoryResolution) -> Option<String> {
    let StartDirectoryResolution::Missing { configured } = resolution else {
        return None;
    };
    let already = session
        .start_directory_state()
        .notice_taken
        .swap(true, Ordering::SeqCst);
    (!already).then(|| configured.clone())
}

/// Für `open_terminal`: die Zeile, die nach dem Shell-Start sichtbar ins PTY
/// geschrieben wird (`None`, wenn nichts konfiguriert oder das Verzeichnis
/// fehlt), und ggf. der einmalige Hinweis.
pub(crate) async fn terminal_start(session: &Session) -> (Option<String>, Option<String>) {
    let resolution = resolve(session).await;
    let command = match resolution {
        StartDirectoryResolution::Found { configured } => {
            Some(start_directory_cd_command(configured))
        }
        _ => None,
    };
    (command, take_missing_notice(session, resolution))
}

/// Ergebnis von `open_terminal` für das Frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStartDto {
    /// s. [`StartDirectoryDto::missing_directory`].
    pub missing_directory: Option<String>,
}

/// Öffnet die interaktive Shell dieser Sitzung und wechselt — falls ein
/// vorhandenes Startverzeichnis konfiguriert ist — sichtbar dorthin, indem
/// nach dem Start der Login-Shell die `cd`-Zeile ins PTY geschrieben wird
/// (Issue-Entscheidung 1). Die Shell-Anfrage selbst bleibt unverändert.
///
/// Scheitert nur, wenn die Shell sich nicht öffnen lässt — wie bisher. Ein
/// fehlendes Startverzeichnis ist kein Fehler, sondern ein Hinweis im
/// Ergebnis; scheitert das Schreiben der `cd`-Zeile, bleibt die Shell im
/// Home und es wird nur protokolliert.
pub async fn open_shell_in_start_directory(
    session: &Session,
    size: PtySize,
) -> Result<(Box<dyn InteractiveShell>, TerminalStartDto), SshError> {
    let mut shell = {
        let mut transport = session.transport.lock().await;
        transport.open_shell(size).await?
    };
    // Erst nach dem Öffnen: die Prüfung braucht den Transport-Lock für den
    // SFTP-Kanal selbst, und der Hinweis soll nur verbraucht werden, wenn es
    // überhaupt ein Terminal gibt, das ihn auslöst.
    let (command, missing_directory) = terminal_start(session).await;
    if let Some(command) = command {
        if let Err(err) = shell.write(command.as_bytes()).await {
            tracing::warn!(
                server_id = %session.server_id.0,
                code = err.code(),
                "writing the start directory cd line to the terminal failed",
            );
        }
    }
    Ok((shell, TerminalStartDto { missing_directory }))
}

/// Für den Dateibrowser: Startpfad und ggf. der einmalige Hinweis.
pub async fn file_browser_start(session: &Session) -> StartDirectoryDto {
    let resolution = resolve(session).await;
    let path = match resolution {
        StartDirectoryResolution::Found { configured } => start_directory_sftp_path(configured),
        _ => ".".to_string(),
    };
    StartDirectoryDto {
        path,
        missing_directory: take_missing_notice(session, resolution),
    }
}

/// Test-Stubs, auch für die Tests des KI-Ausführungspfads (die prüfen, dass
/// ein gesetztes Startverzeichnis dort nichts ändert).
#[cfg(test)]
pub(crate) mod test_stubs {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex as StdMutex};

    use async_trait::async_trait;
    use ssh_manager_core::ssh::{RemoteEntry, SftpSession, SshError};

    /// SFTP-Stub, der nur `stat` kennt: Verzeichnisse aus `dirs`, Dateien
    /// aus `files`, alles andere „nicht gefunden". Zählt die `stat`-Aufrufe.
    #[derive(Clone, Default)]
    pub(crate) struct DirStub {
        dirs: HashSet<String>,
        files: HashSet<String>,
        pub(crate) stats: Arc<StdMutex<Vec<String>>>,
    }

    #[async_trait]
    impl SftpSession for DirStub {
        async fn list_dir(&mut self, _: &str) -> Result<Vec<RemoteEntry>, SshError> {
            unreachable!()
        }
        async fn read_file(&mut self, _: &str) -> Result<Vec<u8>, SshError> {
            unreachable!()
        }
        async fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), SshError> {
            unreachable!()
        }
        async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
            self.stats.lock().unwrap().push(path.to_string());
            let is_dir = self.dirs.contains(path);
            if !is_dir && !self.files.contains(path) {
                return Err(SshError::ChannelError(format!("not found: {path}")));
            }
            Ok(RemoteEntry {
                name: path.to_string(),
                path: path.to_string(),
                is_dir,
                size: 0,
                permissions: 0o755,
                modified: None,
                uid: None,
                gid: None,
                owner: None,
                group: None,
            })
        }
        async fn lstat(&mut self, _: &str) -> Result<RemoteEntry, SshError> {
            unreachable!()
        }
        async fn remove(&mut self, _: &str) -> Result<(), SshError> {
            unreachable!()
        }
        async fn rename(&mut self, _: &str, _: &str) -> Result<(), SshError> {
            unreachable!()
        }
        async fn create_dir(&mut self, _: &str) -> Result<(), SshError> {
            unreachable!()
        }
        async fn remove_dir(&mut self, _: &str) -> Result<(), SshError> {
            unreachable!()
        }
        async fn set_permissions(&mut self, _: &str, _: u32) -> Result<(), SshError> {
            unreachable!()
        }
    }

    pub(crate) fn stub(dirs: &[&str], files: &[&str]) -> DirStub {
        DirStub {
            dirs: dirs.iter().map(|d| d.to_string()).collect(),
            files: files.iter().map(|f| f.to_string()).collect(),
            stats: Arc::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use async_trait::async_trait;
    use ssh_manager_core::ssh::{CommandOutput, SshTransport};

    use super::test_stubs::{stub, DirStub};
    use super::*;
    use crate::test_support::session_with_transport;

    type Written = Arc<StdMutex<Vec<u8>>>;

    /// Transport ohne SFTP (Default von `open_sftp`) — ein Test, der keinen
    /// SFTP-Kanal vorbelegt, sieht damit „SFTP nicht verfügbar". `open_shell`
    /// liefert eine Shell, die alles Geschriebene in `written` festhält.
    struct ShellOnlyTransport {
        written: Written,
    }

    struct RecordingShell {
        written: Written,
    }

    #[async_trait]
    impl InteractiveShell for RecordingShell {
        async fn write(&mut self, data: &[u8]) -> Result<(), SshError> {
            self.written.lock().unwrap().extend_from_slice(data);
            Ok(())
        }
        async fn read(&mut self) -> Result<Vec<u8>, SshError> {
            unreachable!()
        }
        async fn resize(&mut self, _: PtySize) -> Result<(), SshError> {
            unreachable!()
        }
    }

    #[async_trait]
    impl SshTransport for ShellOnlyTransport {
        async fn execute(&mut self, _: &str) -> Result<CommandOutput, SshError> {
            unreachable!("die Startverzeichnis-Prüfung führt nie ein Kommando aus")
        }
        async fn open_shell(&mut self, _: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
            Ok(Box::new(RecordingShell {
                written: self.written.clone(),
            }))
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    async fn session_recording(
        configured: Option<&str>,
        sftp: Option<DirStub>,
    ) -> (Session, Written) {
        let written = Written::default();
        let session = session_with_transport(Box::new(ShellOnlyTransport {
            written: written.clone(),
        }))
        .with_start_directory(configured.map(str::to_string));
        if let Some(sftp) = sftp {
            session.set_sftp_for_tests(Box::new(sftp)).await;
        }
        (session, written)
    }

    async fn session(configured: Option<&str>, sftp: Option<DirStub>) -> Session {
        session_recording(configured, sftp).await.0
    }

    fn written_text(written: &Written) -> String {
        String::from_utf8(written.lock().unwrap().clone()).unwrap()
    }

    const SIZE: PtySize = PtySize { cols: 80, rows: 24 };

    #[tokio::test]
    async fn terminal_writes_visible_cd_line_into_the_shell() {
        let (session, written) = session_recording(
            Some("/srv/it's my app"),
            Some(stub(&["/srv/it's my app"], &[])),
        )
        .await;
        let (_shell, dto) = open_shell_in_start_directory(&session, SIZE).await.unwrap();
        assert_eq!(written_text(&written), "cd -- '/srv/it'\\''s my app'\r");
        assert_eq!(dto.missing_directory, None);
    }

    #[tokio::test]
    async fn terminal_without_start_directory_writes_nothing() {
        let (session, written) = session_recording(None, Some(stub(&[], &[]))).await;
        let (_shell, dto) = open_shell_in_start_directory(&session, SIZE).await.unwrap();
        assert_eq!(written_text(&written), "");
        assert_eq!(dto.missing_directory, None);
    }

    #[tokio::test]
    async fn terminal_with_missing_start_directory_stays_home_and_reports_it_once() {
        let (session, written) = session_recording(Some("/gone"), Some(stub(&[], &[]))).await;
        let (_shell, dto) = open_shell_in_start_directory(&session, SIZE).await.unwrap();
        assert_eq!(written_text(&written), "");
        assert_eq!(dto.missing_directory.as_deref(), Some("/gone"));
        // Ein zweites Terminal (z. B. nach Neuladen) zeigt keinen zweiten Hinweis.
        let (_shell, dto) = open_shell_in_start_directory(&session, SIZE).await.unwrap();
        assert_eq!(dto.missing_directory, None);
    }

    #[tokio::test]
    async fn not_set_behaves_as_before_and_never_touches_sftp() {
        let sftp = stub(&[], &[]);
        let stats = sftp.stats.clone();
        let session = session(None, Some(sftp)).await;

        assert_eq!(terminal_start(&session).await, (None, None));
        assert_eq!(
            file_browser_start(&session).await,
            StartDirectoryDto {
                path: ".".to_string(),
                missing_directory: None
            }
        );
        assert!(stats.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn existing_absolute_directory_is_used_by_terminal_and_browser() {
        let sftp = stub(&["/srv/it's my app"], &[]);
        let session = session(Some("/srv/it's my app"), Some(sftp)).await;

        assert_eq!(
            terminal_start(&session).await,
            (Some("cd -- '/srv/it'\\''s my app'\r".to_string()), None)
        );
        assert_eq!(
            file_browser_start(&session).await,
            StartDirectoryDto {
                path: "/srv/it's my app".to_string(),
                missing_directory: None
            }
        );
    }

    #[tokio::test]
    async fn home_relative_directory_is_checked_relative_to_home() {
        let sftp = stub(&["./projects"], &[]);
        let stats = sftp.stats.clone();
        let session = session(Some("~/projects"), Some(sftp)).await;

        assert_eq!(file_browser_start(&session).await.path, "./projects");
        assert_eq!(
            terminal_start(&session).await.0.as_deref(),
            Some("cd -- ~/'projects'\r")
        );
        assert_eq!(*stats.lock().unwrap(), vec!["./projects".to_string()]);
    }

    #[tokio::test]
    async fn missing_directory_falls_back_to_home_with_exactly_one_notice() {
        let sftp = stub(&[], &[]);
        let stats = sftp.stats.clone();
        let session = session(Some("/does/not/exist"), Some(sftp)).await;

        // Terminal fragt zuerst: kein `cd`, aber der Hinweis.
        assert_eq!(
            terminal_start(&session).await,
            (None, Some("/does/not/exist".to_string()))
        );
        // Der Browser startet im Home, ohne zweiten Hinweis.
        assert_eq!(
            file_browser_start(&session).await,
            StartDirectoryDto {
                path: ".".to_string(),
                missing_directory: None
            }
        );
        assert_eq!(file_browser_start(&session).await.missing_directory, None);
        // Genau eine Prüfung pro Sitzung.
        assert_eq!(stats.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn notice_goes_to_whoever_asks_first() {
        let session = session(Some("/gone"), Some(stub(&[], &[]))).await;
        assert_eq!(
            file_browser_start(&session)
                .await
                .missing_directory
                .as_deref(),
            Some("/gone")
        );
        assert_eq!(terminal_start(&session).await, (None, None));
    }

    #[tokio::test]
    async fn a_file_is_not_a_start_directory() {
        let session = session(Some("/etc/hosts"), Some(stub(&[], &["/etc/hosts"]))).await;
        assert_eq!(
            terminal_start(&session).await,
            (None, Some("/etc/hosts".to_string()))
        );
    }

    #[tokio::test]
    async fn unavailable_sftp_counts_as_missing_and_does_not_fail() {
        // Kein SFTP-Kanal gesetzt; der Mock-Transport kann keinen öffnen.
        let session = session(Some("/srv"), None).await;
        assert_eq!(
            file_browser_start(&session).await,
            StartDirectoryDto {
                path: ".".to_string(),
                missing_directory: Some("/srv".to_string())
            }
        );
    }
}
