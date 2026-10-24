use async_trait::async_trait;
use tokio::sync::{oneshot, watch};

use super::error::SshError;
use super::types::{CommandOutput, ExecOutcome, ExecOutputSink, PtySize, RemoteEntry};

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
    /// Wie [`execute`](Self::execute), aber abbrechbar (Spec 0027): löst
    /// `cancel`, während das Kommando noch läuft (z. B. `journalctl -f`,
    /// das nie von selbst endet), schließt die Implementierung den
    /// Exec-Kanal aktiv und liefert die bis dahin gesammelte Ausgabe mit
    /// `cancelled: true` zurück, statt weiter zu warten. Default-
    /// Implementierung ignoriert `cancel` und delegiert unverändert an
    /// `execute` — bestehende Implementierungen/Mocks (Tests) bleiben ohne
    /// Anpassung lauffähig; nur `RusshTransport` überschreibt sie echt.
    async fn execute_cancellable(
        &mut self,
        command: &str,
        _cancel: oneshot::Receiver<()>,
    ) -> Result<ExecOutcome, SshError> {
        Ok(ExecOutcome {
            output: self.execute(command).await?,
            cancelled: false,
        })
    }
    /// Wie [`execute_with_stdin`](Self::execute_with_stdin), aber abbrechbar
    /// — s. [`execute_cancellable`](Self::execute_cancellable)-Doc-
    /// Kommentar, hier für den `sudo -S`-Stdin-Pfad (Spec 0018).
    async fn execute_with_stdin_cancellable(
        &mut self,
        command: &str,
        stdin: &[u8],
        _cancel: oneshot::Receiver<()>,
    ) -> Result<ExecOutcome, SshError> {
        Ok(ExecOutcome {
            output: self.execute_with_stdin(command, stdin).await?,
            cancelled: false,
        })
    }
    /// Like [`execute_cancellable`](Self::execute_cancellable) /
    /// [`execute_with_stdin_cancellable`](Self::execute_with_stdin_cancellable)
    /// (chosen by `stdin`), but additionally sends every output chunk to
    /// `sink` while the command runs (issue #325, live output in the chat).
    /// The returned [`ExecOutcome`] is identical to the one of the
    /// non-streaming method: the stream is display-only.
    ///
    /// Default: no live chunks, delegates unchanged — existing
    /// implementations and test mocks keep working; `RusshTransport` and
    /// `LocalTransport` override it.
    async fn execute_streaming(
        &mut self,
        command: &str,
        stdin: Option<&[u8]>,
        cancel: oneshot::Receiver<()>,
        sink: ExecOutputSink,
    ) -> Result<ExecOutcome, SshError> {
        drop(sink);
        match stdin {
            Some(stdin) => {
                self.execute_with_stdin_cancellable(command, stdin, cancel)
                    .await
            }
            None => self.execute_cancellable(command, cancel).await,
        }
    }
    /// Issue #328: split form of [`execute_streaming`](Self::execute_streaming).
    /// Opens the exec channel (and writes `stdin`) while the caller holds the
    /// transport, and returns a [`RunningExec`] that no longer needs the
    /// transport: the caller can release its lock while the command runs, so
    /// file browser, terminal and disconnect of the same session are not
    /// blocked. `Ok(None)` (default) means "not supported, use
    /// `execute_streaming`"; `RusshTransport` and `LocalTransport` override it.
    async fn start_exec(
        &mut self,
        command: &str,
        stdin: Option<&[u8]>,
    ) -> Result<Option<Box<dyn RunningExec>>, SshError> {
        let _ = (command, stdin);
        Ok(None)
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
    /// Spec 0067, Teil A: SFTP über einen Exec-Kanal statt über das
    /// `sftp`-Subsystem — `command` startet einen SFTP-Server (z. B. `sudo
    /// -n /usr/lib/openssh/sftp-server`), dessen stdin/stdout das
    /// SFTP-Protokoll trägt. Ein zweiter, eigener Kanal neben
    /// [`open_sftp`](Self::open_sftp). Der Transport kennt kein sudo — das
    /// Kommando baut `crate::ssh::elevated`. Default: nicht unterstützt
    /// (u. a. der lokale Pseudo-Server).
    async fn open_sftp_via_exec(
        &mut self,
        command: &str,
    ) -> Result<Box<dyn SftpSession>, SshError> {
        let _ = command;
        Err(SshError::ChannelError(
            "Erhöhter Dateizugriff wird von diesem Transport nicht unterstützt".to_string(),
        ))
    }
    async fn disconnect(&mut self) -> Result<(), SshError>;

    /// Testhook (Spec 0043, Fund A; Spec 0044 für den lokalen Pseudo-
    /// Server): erlaubt es Tests, den Exec-Output-Cap (Default
    /// `ssh_transport`s `MAX_STREAM_OUTPUT_BYTES`, 2 MB) auf einen kleinen
    /// Wert zu setzen, ohne wirklich Megabyte-Nutzlasten erzeugen zu
    /// müssen. Kein Nutzer-Bedienknopf (Spec 0043, Abschnitt 7: eine
    /// interne Konstante mit klarer Benennung reicht) — Default-
    /// Implementierung ist ein No-op, bestehende `SshTransport`-Mocks
    /// brauchen dafür keine Anpassung; `RusshTransport` UND
    /// `LocalTransport` überschreiben sie echt (beide auf denselben
    /// Default nach oben geklemmt, s. jeweiliger `set_max_output_bytes`-
    /// Kommentar).
    fn set_max_output_bytes(&mut self, _limit: usize) {}

    /// Issue #353: a watch that turns to `Some` once the connection is
    /// gone (keepalive timeout, server disconnect, stream error, local
    /// close). Lets the application notice a dead connection without an
    /// open terminal. Default: `None` (the transport cannot tell).
    fn connection_loss_watch(&self) -> Option<ConnectionLossWatch> {
        None
    }
}

/// Issue #353: why a connection ended; `reason` is user-facing text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionLoss {
    pub reason: Option<String>,
}

/// Receiver side of [`SshTransport::connection_loss_watch`].
pub type ConnectionLossWatch = watch::Receiver<Option<ConnectionLoss>>;

/// A started exec channel that runs independently of the transport it was
/// opened from (issue #328, see [`SshTransport::start_exec`]).
#[async_trait]
pub trait RunningExec: Send {
    /// Waits for the command to end (or for `cancel`), forwarding output
    /// chunks to `sink`; same result semantics as
    /// [`SshTransport::execute_streaming`].
    async fn run(
        self: Box<Self>,
        cancel: oneshot::Receiver<()>,
        sink: ExecOutputSink,
    ) -> Result<ExecOutcome, SshError>;
}

/// Issue #622: Antwort des Aufrufers auf ein gelesenes Stück.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkControl {
    Continue,
    Stop,
}

/// Issue #622: Was ein Upload-Aufrufer für das nächste Stück liefert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkSource {
    /// Die ersten `n` Bytes des Puffers sind das nächste Stück.
    Data(usize),
    /// Quelle zu Ende.
    Eof,
    /// Abbruch gewünscht (Nutzer oder Widerruf).
    Stop,
}

/// Issue #622: wie ein chunked Transfer endete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferEnd {
    Completed,
    Stopped,
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
///
/// **Keine Default-Rümpfe in diesem Trait** (Spec 0085, A1.1, spec-reviewer
/// Runde 1/2): Der erhöhte Dateibrowser-Kanal wird über einen Wrapper benutzt,
/// der vor **jeder einzelnen** Operation prüft, ob der erhöhte Modus noch gilt
/// (`app_shell::commands::elevation::BrowserSftp`). Dass jede Operation dort
/// einzeln auftaucht und einzeln geprüft wird, erzwingt der Compiler — aber
/// nur, solange jede Methode hier **ohne** Rumpf deklariert ist. Eine Methode
/// mit Default-Rumpf (wie sie `SshTransport` oben durchaus hat) käme beim
/// Wrapper ohne Eintrag durch; niemand müsste ihn beim Ergänzen noch ansehen.
/// Die Prüfung selbst wäre damit nicht umgangen (ein Default-Rumpf erreicht
/// den inneren Kanal nicht, er kann nur andere — geprüfte — Trait-Methoden
/// aufrufen), aber das Geländer wäre weg. Wer hier eine Methode ergänzt,
/// deklariert sie also ohne Rumpf und trägt sie beim Wrapper nach.
#[async_trait]
pub trait SftpSession: Send {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError>;
    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError>;
    /// Wie `read_file`, liest aber **höchstens** `max_bytes` Bytes und hört
    /// dann auf (Issue #357): der Speicher hält nie mehr als `max_bytes`,
    /// auch wenn die Datei viel größer ist oder ihre Größe unbekannt war.
    /// Wer „größer als N" erkennen will, übergibt `N + 1` und prüft die
    /// Länge des Ergebnisses. `progress` bekommt nach jedem gelesenen Stück
    /// die bisher gelesene Byte-Zahl. Abbrechen heißt, den Aufruf fallen zu
    /// lassen (Drop des Futures).
    async fn read_file_capped(
        &mut self,
        path: &str,
        max_bytes: u64,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Vec<u8>, SshError>;
    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError>;
    /// Issue #622: liest `path` in Stücken von höchstens `chunk_size` Bytes
    /// und übergibt jedes an `on_chunk`; der Speicher hält nie mehr als ein
    /// Stück. Antwortet `on_chunk` mit [`ChunkControl::Stop`], endet das Lesen
    /// sofort mit `Ok(`[`TransferEnd::Stopped`]`)` — vor dem nächsten Stück.
    async fn read_file_chunked(
        &mut self,
        path: &str,
        chunk_size: usize,
        on_chunk: &mut (dyn for<'a> FnMut(&'a [u8]) -> ChunkControl + Send),
    ) -> Result<TransferEnd, SshError>;
    /// Issue #622: Gegenstück zu [`Self::read_file_chunked`] — legt `path` an
    /// (bzw. überschreibt es) und schreibt, was `next_chunk` liefert. Der
    /// Aufrufer bekommt einen Puffer der Größe `chunk_size` und meldet, wie
    /// viele Bytes davon gültig sind ([`ChunkSource`]). Bei
    /// [`ChunkSource::Stop`] bleibt die bis dahin geschriebene Datei stehen
    /// (der Aufrufer räumt sie auf) und das Ergebnis ist
    /// `Ok(`[`TransferEnd::Stopped`]`)`.
    async fn write_file_chunked(
        &mut self,
        path: &str,
        chunk_size: usize,
        next_chunk: &mut (dyn for<'a> FnMut(&'a mut [u8]) -> ChunkSource + Send),
    ) -> Result<TransferEnd, SshError>;
    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError>;
    /// Wie `stat`, folgt aber KEINEM Symlink am Zielpfad selbst (SFTP
    /// `LSTAT`) — meldet für einen Symlink dessen eigenen Typ (`is_dir ==
    /// false`, unabhängig davon, worauf er zeigt). Spec 0054, Teil 3: die
    /// Grundlage dafür, dass rekursives Löschen/chmod nie versehentlich in
    /// ein Symlink-Ziel außerhalb des sichtbaren Baums hinein traversiert
    /// (s. `commands::delete_recursive`/`chmod_recursive`, die den
    /// Wurzelpfad damit statt mit `stat` prüfen).
    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError>;
    async fn remove(&mut self, path: &str) -> Result<(), SshError>;
    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError>;
    /// Issue #625: benennt `from` atomar nach `to` um und **ersetzt** dabei
    /// ein vorhandenes Ziel (`posix-rename@openssh.com`). `Ok(true)`: der
    /// Server hat es ausgeführt. `Ok(false)`: der Server bietet die
    /// Erweiterung nicht an — es wurde **nichts** gesendet, der Aufrufer
    /// weicht auf „Ziel entfernen, dann `rename`" aus.
    async fn posix_rename(&mut self, from: &str, to: &str) -> Result<bool, SshError>;
    async fn create_dir(&mut self, path: &str) -> Result<(), SshError>;
    /// Legt eine **leere** Datei an und überschreibt nie etwas (Issue #570):
    /// SFTP `OPEN` mit CREATE|EXCL, ein vorhandener Name (Datei, Ordner,
    /// Symlink) scheitert atomar mit [`SshError::SftpAlreadyExists`].
    async fn create_file_exclusive(&mut self, path: &str) -> Result<(), SshError>;
    /// Löscht ein **leeres** Verzeichnis (SFTP `RMDIR`) — Gegenstück zu
    /// `remove()`, das nur auf Dateien wirkt (Spec 0054, Teil 3: rekursives
    /// Ordner-Löschen im Aufrufer baut darauf auf, indem zuerst alle
    /// Dateien und tiefer liegenden Verzeichnisse entfernt werden, bevor
    /// `remove_dir` von unten nach oben aufgerufen wird).
    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError>;
    /// Setzt die reinen Unix-Rechte-Bits (`0o755`-Stil, wie
    /// `RemoteEntry::permissions`) — Spec 0054, Teil 3 (chmod). Ändert
    /// bewusst nur die Rechte, nicht Eigentümer/Gruppe/Zeitstempel (SFTP
    /// `SETSTAT` könnte das, aber der chmod-Dialog braucht nur dieses eine
    /// Feld).
    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError>;
    /// Sets owner and group by numeric id (issue #654, chown): SFTP `SETSTAT`
    /// with uid and gid, which the protocol only accepts as a pair. The
    /// caller resolves names and fills in the side it does not change. The
    /// server decides whether the change is allowed.
    async fn set_owner(&mut self, path: &str, uid: u32, gid: u32) -> Result<(), SshError>;
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
