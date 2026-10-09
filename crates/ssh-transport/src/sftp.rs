//! `russh-sftp`-gestützte Implementierung von `SftpSession` (Spec 0020,
//! Abschnitt 3) — läuft über einen `sftp`-Subsystem-Channel derselben
//! `SshTransport`-Verbindung, kein zweiter Verbindungsaufbau.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use russh::client::Handle;
use russh_sftp::client::error::Error as RusshSftpError;
use russh_sftp::client::SftpSession as RusshSftpClient;
use russh_sftp::protocol::{FileAttributes, StatusCode};
use tokio::io::AsyncWriteExt;

use ssh_manager_core::ssh::{RemoteEntry, SftpSession, SshError};

use crate::handler::ClientHandler;

pub struct RusshSftpSession {
    inner: RusshSftpClient,
    session: SessionProbe,
}

impl RusshSftpSession {
    pub(crate) fn new(inner: RusshSftpClient, session: SessionProbe) -> Self {
        Self { inner, session }
    }

    /// Issue #155: [`map_sftp_error`] mit dem aktuellen Zustand der
    /// SSH-Sitzung, auf der dieser SFTP-Kanal läuft.
    fn map_error(&self, path: &str, e: RusshSftpError) -> SshError {
        map_sftp_error(path, e, self.session.is_closed())
    }

    /// Issue #155: wie [`Self::map_error`], für die `io::Error` aus dem
    /// Schreiben über einen geöffneten Datei-Handle (`write_all`/`shutdown`).
    fn map_write_error(&self, path: &str, e: std::io::Error) -> SshError {
        let message = format!("SFTP-Schreibfehler bei '{path}': {e}");
        if is_channel_gone_io(&e) && self.session.is_closed() {
            SshError::SessionClosed(message)
        } else {
            SshError::ChannelError(message)
        }
    }
}

/// Issue #155: Fragt ab, ob die SSH-Sitzung, auf der ein SFTP-Kanal läuft,
/// beendet ist.
///
/// Hält bewusst nur einen [`Weak`]-Verweis auf den `russh`-Handle: ein
/// offener SFTP-Kanal darf die Sitzung nicht über die Lebensdauer ihres
/// Transports hinaus am Leben halten (das Droppen des Handles beendet die
/// Sitzung). Ist der Transport schon weg, gilt die Sitzung als beendet.
///
/// Warum dieses Signal zusätzlich zur Fehlerart nötig ist: `russh-sftp`
/// meldet einen geschlossenen Kanal immer gleich („session closed",
/// „sender dropped", …) — egal, ob die ganze SSH-Sitzung weg ist oder nur
/// dieser eine Kanal endete (z. B. ein von sudo abgelehnter
/// `sftp-server`-Start über einen Exec-Kanal, Spec 0067). Nur im ersten Fall
/// ist `SSH_SESSION_CLOSED` richtig. `russh` schließt beim Sitzungsende
/// zuerst die Nachrichten-Warteschlange des Handles (`is_closed()` wird
/// `true`) und gibt erst danach die Kanäle frei — wenn der SFTP-Kanal wegen
/// des Sitzungsendes zugeht, ist `is_closed()` also schon gesetzt.
#[derive(Clone)]
pub(crate) struct SessionProbe(Weak<Handle<ClientHandler>>);

impl SessionProbe {
    pub(crate) fn new(handle: &Arc<Handle<ClientHandler>>) -> Self {
        Self(Arc::downgrade(handle))
    }

    fn is_closed(&self) -> bool {
        self.0.upgrade().is_none_or(|handle| handle.is_closed())
    }
}

/// Issue #155: die Meldungen, mit denen `russh-sftp` (2.4) einen
/// geschlossenen Kanal anzeigt — `UnexpectedBehavior("session closed")`
/// (Senden auf einen beendeten Kanal), `UnexpectedBehavior("sender
/// dropped")` (Antwort kam nie, weil der Lese-Task endete) und die aus
/// `mpsc::SendError`/`oneshot::RecvError` erzeugten Varianten.
fn is_channel_gone_message(msg: &str) -> bool {
    msg == "session closed"
        || msg == "sender dropped"
        || msg.starts_with("SendError")
        || msg.starts_with("RecvError")
}

/// Issue #155: Kann dieser Fehler von einem geschlossenen Kanal stammen?
/// `IO(_)` trägt nur noch den Text des ursprünglichen `io::Error` (die
/// `ErrorKind` ist verloren) und kommt aus dem Kanal-Stream selbst.
///
/// `Timeout` gehört dazu, weil `russh-sftp` (2.4) eine Anfrage, die beim
/// Ende des Kanals noch unterwegs ist, nie abbricht: die Tabelle offener
/// Anfragen teilt sich der beendete Lese-Task mit der Sitzung, die
/// Antwort-Sender bleiben also bestehen, und die Anfrage endet erst mit
/// ihrer Frist. Erst die **nächste** Anfrage bekommt „session closed".
///
/// Allein reicht keiner dieser Fehler für `SessionClosed` — s.
/// [`SessionProbe`]: ein `Timeout` bei lebender Sitzung ist eine echte
/// Zeitüberschreitung.
fn is_channel_gone(e: &RusshSftpError) -> bool {
    match e {
        RusshSftpError::IO(_) | RusshSftpError::Timeout => true,
        RusshSftpError::UnexpectedBehavior(msg) => is_channel_gone_message(msg),
        _ => false,
    }
}

/// Issue #155: dasselbe für die `io::Error`, die ein `russh-sftp`-Datei-
/// Handle beim Schreiben liefert — dort ist der Client-Fehler schon in
/// einen `io::Error` mit dessen Text verpackt, eine verlorene Bestätigung
/// kommt als `BrokenPipe` („write channel closed"). Die Texte sind die
/// `Display`-Ausgaben der Varianten aus [`is_channel_gone`].
fn is_channel_gone_io(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        return true;
    }
    let msg = e.to_string();
    msg.starts_with("I/O: ") || msg == "Timeout" || is_channel_gone_message(&msg)
}

/// Issue #155 (Spec 0069, Teil A3): Ist die Sitzung, auf der eine
/// SFTP-Operation lief, weg?
///
/// - Die SFTP-Statuscodes `NoConnection`/`ConnectionLost` sagen das selbst.
/// - Ein Fehler, der einen geschlossenen Kanal anzeigt
///   ([`is_channel_gone`]), zählt nur, wenn auch die SSH-Sitzung beendet ist
///   (`session_closed`, s. [`SessionProbe`]). Endete nur der Kanal, bleibt
///   es ein Kanal-Fehler.
///
/// Alles andere (übrige Statuscodes, `Limited`, `UnexpectedPacket`,
/// sonstiges Protokollverhalten) ist nie „Sitzung weg".
pub(crate) fn is_session_gone(e: &RusshSftpError, session_closed: bool) -> bool {
    match e {
        RusshSftpError::Status(status) => matches!(
            status.status_code,
            StatusCode::NoConnection | StatusCode::ConnectionLost
        ),
        other => session_closed && is_channel_gone(other),
    }
}

/// Wandelt einen `russh_sftp::client::error::Error` in ein [`SshError`].
/// `PermissionDenied` bekommt bewusst eine eigene [`SshError`]-Variante
/// (Spec 0020, Abschnitt 4.3: App-Ebene muss diesen einen Fall zuverlässig
/// vom allgemeinen Fehlerfall unterscheiden können, für den Sudo-Rechte-
/// Fallback). Ist die Sitzung weg ([`is_session_gone`], Issue #155), wird
/// es [`SshError::SessionClosed`] (`SSH_SESSION_CLOSED`, Spec 0069, Teil
/// A3) — alles andere landet in `ChannelError` mit der Original-
/// Fehlermeldung, analog zu `crate::error::map_russh_error`. Beide tragen
/// den Pfad im Text.
fn map_sftp_error(path: &str, e: RusshSftpError, session_closed: bool) -> SshError {
    match &e {
        RusshSftpError::Status(status) if status.status_code == StatusCode::PermissionDenied => {
            SshError::SftpPermissionDenied(format!("{path}: {e}"))
        }
        _ if is_session_gone(&e, session_closed) => {
            SshError::SessionClosed(format!("SFTP-Fehler bei '{path}': {e}"))
        }
        _ => SshError::ChannelError(format!("SFTP-Fehler bei '{path}': {e}")),
    }
}

/// Issue #155: Abbildung eines gescheiterten SFTP-Handshakes auf einem
/// bereits offenen Kanal (`open_sftp`, `open_sftp_via_exec`). Ist die
/// Sitzung weg ([`is_session_gone`]), wird es `SessionClosed`; sonst bleibt
/// es wie bisher ein `ChannelError` mit `context` als Präfix — darunter
/// auch der von sudo abgelehnte Exec-Start, bei dem nur der Kanal endet.
pub(crate) fn map_sftp_init_error(
    context: &str,
    e: RusshSftpError,
    session_closed: bool,
) -> SshError {
    if is_session_gone(&e, session_closed) {
        SshError::SessionClosed(format!("{context}: {e}"))
    } else {
        SshError::ChannelError(format!("{context}: {e}"))
    }
}

/// `permissions` liefert `russh-sftp` als rohen `st_mode`-Wert inkl.
/// Dateityp-Bits — `RemoteEntry::permissions` soll nur die reinen
/// Rechte-Bits tragen (s. dortiger Doc-Kommentar), daher hier maskiert.
fn remote_entry(name: String, path: String, metadata: &FileAttributes) -> RemoteEntry {
    RemoteEntry {
        name,
        path,
        is_dir: metadata.is_dir(),
        size: metadata.len(),
        permissions: metadata.permissions.unwrap_or(0) & 0o7777,
        modified: metadata.modified().ok().map(DateTime::<Utc>::from),
        uid: metadata.uid,
        gid: metadata.gid,
        owner: metadata.user.clone(),
        group: metadata.group.clone(),
    }
}

#[async_trait]
impl SftpSession for RusshSftpSession {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        let entries = self
            .inner
            .read_dir(path)
            .await
            .map_err(|e| self.map_error(path, e))?;
        Ok(entries
            .map(|entry| {
                let metadata = entry.metadata();
                remote_entry(entry.file_name(), entry.path(), &metadata)
            })
            .collect())
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.inner
            .read(path)
            .await
            .map_err(|e| self.map_error(path, e))
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        // Bewusst nicht `SftpSession::write()`: dessen Implementierung öffnet
        // nur mit `OpenFlags::WRITE` (kein `CREATE`) und scheitert deshalb
        // mit "No such file", sobald die Zieldatei noch nicht existiert —
        // für `write_file` (erstellt oder überschreibt, Spec 0020, Abschnitt
        // 3/4.2) das falsche Verhalten. `create()` (CREATE|TRUNCATE|WRITE)
        // deckt beide Fälle korrekt ab.
        let mut file = self
            .inner
            .create(path)
            .await
            .map_err(|e| self.map_error(path, e))?;
        file.write_all(content)
            .await
            .map_err(|e| self.map_write_error(path, e))?;
        // `File::poll_write` reicht Schreibanfragen nur "fire-and-forget"
        // weiter (`write_nowait`) — erst `shutdown()`/`flush()` wartet auf
        // die ausstehenden Bestätigungen. Ohne diesen Aufruf lief der
        // Handle in die `Drop`-Implementierung (`close_nowait`, Antwort
        // nicht abgewartet) — in der Praxis unauffällig, aber kein
        // garantiert abgeschlossener Schreibvorgang, bevor `write_file`
        // zurückkehrt.
        file.shutdown()
            .await
            .map_err(|e| self.map_write_error(path, e))?;
        Ok(())
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        let metadata = self
            .inner
            .metadata(path)
            .await
            .map_err(|e| self.map_error(path, e))?;
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        Ok(remote_entry(name, path.to_string(), &metadata))
    }

    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        let metadata = self
            .inner
            .symlink_metadata(path)
            .await
            .map_err(|e| self.map_error(path, e))?;
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        Ok(remote_entry(name, path.to_string(), &metadata))
    }

    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.inner
            .remove_file(path)
            .await
            .map_err(|e| self.map_error(path, e))
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.inner
            .rename(from, to)
            .await
            .map_err(|e| self.map_error(from, e))
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.inner
            .create_dir(path)
            .await
            .map_err(|e| self.map_error(path, e))
    }

    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.inner
            .remove_dir(path)
            .await
            .map_err(|e| self.map_error(path, e))
    }

    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        let attrs = FileAttributes {
            permissions: Some(mode),
            ..Default::default()
        };
        self.inner
            .set_metadata(path, attrs)
            .await
            .map_err(|e| self.map_error(path, e))
    }
}

#[cfg(test)]
mod map_sftp_error_tests {
    //! Issue #155 (Spec 0069, Teil A3): Einordnung von SFTP-Fehlern, wenn
    //! die Sitzung darunter weg ist. **Gegenbeweis**: vor dem Fix bildete
    //! `map_sftp_error` jeden der „Sitzung weg"-Fälle unten auf
    //! `ChannelError` (`SSH_CHANNEL_ERROR`) ab.
    use std::io::{Error as IoError, ErrorKind};
    use std::sync::Weak;

    use russh_sftp::client::error::Error as RusshSftpError;
    use russh_sftp::protocol::{Status, StatusCode};
    use ssh_manager_core::ssh::SshError;

    use super::{
        is_channel_gone, is_channel_gone_io, map_sftp_error, map_sftp_init_error, SessionProbe,
    };

    const PATH: &str = "/srv/data/report.txt";

    fn status(code: StatusCode) -> RusshSftpError {
        RusshSftpError::Status(Status {
            id: 7,
            status_code: code,
            error_message: format!("{code}"),
            language_tag: "en-US".to_string(),
        })
    }

    /// Ein echter `SendError` aus einem `mpsc`-Kanal, dessen Empfänger weg
    /// ist — so, wie `russh-sftp` ihn per `From` selbst erzeugt.
    fn real_send_error() -> RusshSftpError {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<u8>();
        drop(rx);
        tx.send(1).unwrap_err().into()
    }

    /// Ein echter `RecvError` aus einem `oneshot`, dessen Sender weg ist —
    /// den liefert nur das Abwarten (`Future`), nicht `try_recv`.
    fn real_recv_error() -> RusshSftpError {
        let (tx, rx) = tokio::sync::oneshot::channel::<u8>();
        drop(tx);
        block_on_ready(rx).unwrap_err().into()
    }

    /// Pollt ein sofort fertiges `Future` einmal — ohne Laufzeit und ohne
    /// neue Abhängigkeit.
    fn block_on_ready<F: std::future::Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let mut fut = std::pin::pin!(fut);
        let mut cx = Context::from_waker(Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(out) => out,
            Poll::Pending => panic!("der Empfang sollte sofort fertig sein"),
        }
    }

    /// Jeder Fall, der laut Regel einen geschlossenen Kanal anzeigt.
    fn channel_gone_cases() -> Vec<RusshSftpError> {
        vec![
            RusshSftpError::IO("broken pipe".to_string()),
            RusshSftpError::UnexpectedBehavior("session closed".to_string()),
            RusshSftpError::UnexpectedBehavior("sender dropped".to_string()),
            RusshSftpError::Timeout,
            real_send_error(),
            real_recv_error(),
        ]
    }

    /// Fälle, die nie „Sitzung weg" bedeuten — auch dann nicht, wenn die
    /// Sitzung gerade zufällig beendet ist. (`Timeout` steht in
    /// [`channel_gone_cases`]: bei lebender Sitzung bleibt es
    /// `ChannelError`, s. `test_closed_channel_on_live_session_stays_channel_error`.)
    fn other_cases() -> Vec<RusshSftpError> {
        vec![
            status(StatusCode::NoSuchFile),
            status(StatusCode::Failure),
            status(StatusCode::BadMessage),
            status(StatusCode::OpUnsupported),
            RusshSftpError::Limited("write limit reached".to_string()),
            RusshSftpError::UnexpectedPacket,
            RusshSftpError::UnexpectedBehavior("no file".to_string()),
            RusshSftpError::UnexpectedBehavior("Duplicate version".to_string()),
        ]
    }

    fn assert_session_closed(mapped: &SshError, label: &str) {
        assert!(
            matches!(mapped, SshError::SessionClosed(msg) if msg.contains(PATH)),
            "{label} -> {mapped:?}"
        );
        assert_eq!(mapped.code(), "SSH_SESSION_CLOSED", "{label}");
    }

    fn assert_channel_error(mapped: &SshError, label: &str) {
        assert!(
            matches!(mapped, SshError::ChannelError(msg) if msg.contains(PATH)),
            "{label} -> {mapped:?}"
        );
        assert_eq!(mapped.code(), "SSH_CHANNEL_ERROR", "{label}");
    }

    #[test]
    fn test_real_library_channel_errors_are_recognised() {
        // Schützt die Text-Regel in `is_channel_gone_message` gegen die
        // tatsächlich erzeugten Meldungen.
        assert!(is_channel_gone(&real_send_error()));
        assert!(is_channel_gone(&real_recv_error()));
    }

    #[test]
    fn test_closed_channel_on_closed_session_maps_to_session_closed() {
        for case in channel_gone_cases() {
            let label = format!("{case:?}");
            assert_session_closed(&map_sftp_error(PATH, case, true), &label);
        }
    }

    #[test]
    fn test_connection_status_codes_map_to_session_closed() {
        for code in [StatusCode::NoConnection, StatusCode::ConnectionLost] {
            for session_closed in [true, false] {
                let label = format!("{code:?}, session_closed={session_closed}");
                assert_session_closed(&map_sftp_error(PATH, status(code), session_closed), &label);
            }
        }
    }

    /// Nur der Kanal ist zu, die Sitzung lebt (z. B. `sftp-server` beendet),
    /// oder eine echte Zeitüberschreitung (`Timeout`): bleibt
    /// `ChannelError` mit Pfad, nicht „Verbindung unterbrochen".
    #[test]
    fn test_closed_channel_on_live_session_stays_channel_error() {
        for case in channel_gone_cases() {
            let label = format!("{case:?}");
            assert_channel_error(&map_sftp_error(PATH, case, false), &label);
        }
    }

    #[test]
    fn test_permission_denied_stays_sftp_permission_denied() {
        for session_closed in [true, false] {
            let mapped = map_sftp_error(PATH, status(StatusCode::PermissionDenied), session_closed);
            assert!(
                matches!(&mapped, SshError::SftpPermissionDenied(msg) if msg.contains(PATH)),
                "session_closed={session_closed} -> {mapped:?}"
            );
        }
    }

    #[test]
    fn test_other_errors_stay_channel_error_with_path() {
        for session_closed in [true, false] {
            for case in other_cases() {
                let label = format!("{case:?}, session_closed={session_closed}");
                assert_channel_error(&map_sftp_error(PATH, case, session_closed), &label);
            }
        }
    }

    #[test]
    fn test_handshake_on_closed_session_maps_to_session_closed() {
        for context in [
            "SFTP-Init fehlgeschlagen",
            "SFTP-Init über Exec-Kanal fehlgeschlagen",
        ] {
            for case in channel_gone_cases() {
                let label = format!("{context}: {case:?}");
                let mapped = map_sftp_init_error(context, case, true);
                assert!(
                    matches!(&mapped, SshError::SessionClosed(msg) if msg.starts_with(context)),
                    "{label} -> {mapped:?}"
                );
                assert_eq!(mapped.code(), "SSH_SESSION_CLOSED", "{label}");
            }
        }
    }

    /// Ein von sudo abgelehnter Exec-Start schließt nur den Kanal: bleibt
    /// wie bisher `ChannelError`. Ebenso jeder andere Handshake-Fehler.
    #[test]
    fn test_other_handshake_failures_keep_channel_error() {
        let context = "SFTP-Init über Exec-Kanal fehlgeschlagen";
        let mut cases: Vec<(RusshSftpError, bool)> = channel_gone_cases()
            .into_iter()
            .map(|case| (case, false))
            .collect();
        for session_closed in [true, false] {
            cases.extend(other_cases().into_iter().map(|c| (c, session_closed)));
            cases.push((status(StatusCode::PermissionDenied), session_closed));
        }
        for (case, session_closed) in cases {
            let label = format!("{case:?}, session_closed={session_closed}");
            let mapped = map_sftp_init_error(context, case, session_closed);
            assert!(
                matches!(&mapped, SshError::ChannelError(msg) if msg.starts_with(context)),
                "{label} -> {mapped:?}"
            );
        }
    }

    #[test]
    fn test_write_path_io_errors_from_closed_channel_are_recognised() {
        assert!(is_channel_gone_io(&IoError::new(
            ErrorKind::BrokenPipe,
            "write channel closed"
        )));
        assert!(is_channel_gone_io(&IoError::other("session closed")));
        assert!(is_channel_gone_io(&IoError::other("sender dropped")));
        assert!(is_channel_gone_io(&IoError::other(
            real_send_error().to_string()
        )));
        assert!(is_channel_gone_io(&IoError::other("I/O: early eof")));
        assert!(is_channel_gone_io(&IoError::other(
            RusshSftpError::Timeout.to_string()
        )));
        // Server-Status und unerwartete Antworten beim Schreiben: nie.
        assert!(!is_channel_gone_io(&IoError::other("No space left")));
        assert!(!is_channel_gone_io(&IoError::other(
            "unexpected response packet"
        )));
    }

    /// Ist der Transport schon gedroppt, gilt die Sitzung als beendet.
    #[test]
    fn test_probe_without_transport_reports_closed() {
        assert!(SessionProbe(Weak::new()).is_closed());
    }

    /// SFTP-`VERSION`-Antwort (Protokollversion 3, keine Erweiterungen),
    /// von Hand kodiert: Länge, Typ 2, Version.
    const VERSION_REPLY: [u8; 9] = [0, 0, 0, 5, 2, 0, 0, 0, 3];

    /// Gegen das echte Verhalten von `russh-sftp`, nicht nur gegen
    /// nachgebaute Fehlerwerte: Handshake gelingt, dann endet der Stream
    /// (wie beim Abbruch der SSH-Sitzung). Eine **laufende** Anfrage endet
    /// mit `Timeout`, jede **weitere** mit „session closed" — beide erkennt
    /// die Regel als geschlossenen Kanal.
    #[tokio::test]
    async fn test_real_client_errors_after_stream_end_are_channel_gone() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (client, mut server) = tokio::io::duplex(4096);
        let server_side = tokio::spawn(async move {
            let len = server.read_u32().await.unwrap();
            let mut init = vec![0u8; len as usize];
            server.read_exact(&mut init).await.unwrap();
            server.write_all(&VERSION_REPLY).await.unwrap();
            // Die nächste Anfrage noch annehmen, dann den Stream beenden,
            // ohne zu antworten.
            let len = server.read_u32().await.unwrap();
            let mut request = vec![0u8; len as usize];
            server.read_exact(&mut request).await.unwrap();
        });
        let sftp = russh_sftp::client::SftpSession::new(client)
            .await
            .expect("Handshake mit VERSION-Antwort sollte gelingen");
        sftp.set_timeout(1);

        let in_flight = sftp.metadata(PATH).await.unwrap_err();
        server_side.await.unwrap();
        assert!(
            matches!(in_flight, RusshSftpError::Timeout),
            "{in_flight:?}"
        );
        assert!(is_channel_gone(&in_flight));
        assert_session_closed(&map_sftp_error(PATH, in_flight, true), "in flight");

        // Der Lese-Task hat das Stream-Ende inzwischen gesehen.
        let mut next = sftp.metadata(PATH).await.unwrap_err();
        for _ in 0..20 {
            if !matches!(next, RusshSftpError::Timeout) {
                break;
            }
            next = sftp.metadata(PATH).await.unwrap_err();
        }
        assert!(
            matches!(&next, RusshSftpError::UnexpectedBehavior(msg) if msg == "session closed"),
            "{next:?}"
        );
        assert!(is_channel_gone(&next));
        assert_session_closed(&map_sftp_error(PATH, next, true), "next request");
    }
}
