use async_trait::async_trait;

use super::error::SshError;
use super::types::{CommandOutput, PtySize, RemoteEntry};

/// Offene Verbindung zu einem SSH-Server (Spec 0005, Abschnitt 1/4). Exec-
/// und interaktiver Modus laufen über **denselben** Transport (SSH-
/// Multiplexing über Channels), nicht über separate Neuverbindungen.
///
/// `async fn` über `async-trait`, damit der Trait weiterhin als
/// `Box<dyn SshTransport>` nutzbar bleibt (native `async fn` in Traits ist,
/// Stand der in diesem Workspace verwendeten Rust-Version, nicht
/// dyn-kompatibel) — dasselbe Muster wie `ProfileStore` in `core::profiles`
/// (Spec 0003/0004).
#[async_trait]
pub trait SshTransport: Send + Sync {
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError>;
    /// Wie [`execute`](Self::execute), schreibt aber `stdin` in den
    /// Exec-Channel, bevor auf die Antwort gewartet wird (Spec 0018:
    /// `sudo -S`-Passworteingabe ohne TTY). Default-Implementierung
    /// ignoriert `stdin` und delegiert unverändert an `execute` — bestehende
    /// Implementierungen/Mocks (Tests) bleiben dadurch ohne Anpassung
    /// lauffähig; nur `RusshTransport` überschreibt sie echt.
    async fn execute_with_stdin(
        &mut self,
        command: &str,
        stdin: &[u8],
    ) -> Result<CommandOutput, SshError> {
        let _ = stdin;
        self.execute(command).await
    }
    async fn open_shell(&mut self, size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError>;
    /// Öffnet eine SFTP-Session als weiteren Subsystem-Channel derselben
    /// Verbindung (Spec 0020, Abschnitt 3) — kein zweiter Verbindungsaufbau,
    /// keine erneute Auth/Host-Key-Prüfung. Default-Implementierung liefert
    /// einen Fehler (analog zu `execute_with_stdin`s Default): bestehende
    /// `SshTransport`-Mocks/-Stubs in Tests, die keinen Dateizugriff prüfen,
    /// müssen dafür nicht angepasst werden — nur `RusshTransport` und
    /// gezielte SFTP-Test-Doubles überschreiben sie echt.
    async fn open_sftp(&mut self) -> Result<Box<dyn SftpSession>, SshError> {
        Err(SshError::ChannelError(
            "SFTP wird von diesem Transport nicht unterstützt".to_string(),
        ))
    }
    async fn disconnect(&mut self) -> Result<(), SshError>;
}

/// Dateizugriff über SFTP (Spec 0020, Abschnitt 3) — läuft, wie
/// [`SshTransport::execute`], **nicht** über die Filter-Engine; Kontrolle
/// über SFTP-initiierte Datei-Operationen ist Sache der aufrufenden Ebene
/// (`crate::filter`/App-Kernschleife bilden `ReadRemoteFile`/
/// `WriteRemoteFile` dafür auf Pseudokommandos ab, s. Spec 0020, Abschnitt
/// 4), nicht dieses Traits.
///
/// `Send` (nicht zusätzlich `Sync`): eine SFTP-Session wird pro
/// Server-Session lazy geöffnet und danach exklusiv unter demselben Lock
/// wie `SshTransport` selbst gehalten (nie aus zwei Tasks gleichzeitig
/// angesprochen) — kein `Sync`-Bedarf, anders als `SshTransport` selbst
/// (das als `Arc<Mutex<..>>` zwischen Kommando-Ausführung und Terminal-Aktor
/// geteilt wird).
#[async_trait]
pub trait SftpSession: Send {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError>;
    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError>;
    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError>;
    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError>;
    async fn remove(&mut self, path: &str) -> Result<(), SshError>;
    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError>;
    async fn create_dir(&mut self, path: &str) -> Result<(), SshError>;
}

/// Offene PTY-Shell für den interaktiven Modus (Terminal-Tab, xterm.js im
/// Frontend), Spec 0005 Abschnitt 1/4 — läuft bewusst **nicht** durch die
/// Filter-Engine (Spec 0002), da hier der Nutzer direkt selbst tippt.
#[async_trait]
pub trait InteractiveShell: Send {
    async fn write(&mut self, data: &[u8]) -> Result<(), SshError>;
    /// Blockiert, bis Daten verfügbar sind oder EOF erreicht wird.
    async fn read(&mut self) -> Result<Vec<u8>, SshError>;
    async fn resize(&mut self, size: PtySize) -> Result<(), SshError>;
}
