use async_trait::async_trait;

use super::error::SshError;
use super::types::{CommandOutput, PtySize};

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
    async fn disconnect(&mut self) -> Result<(), SshError>;
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
