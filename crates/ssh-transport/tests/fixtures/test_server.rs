//! In-process `russh`-Test-Server (Spec 0005, Abschnitt 8, zweiter Punkt;
//! Aufgabenstellung Teil 2, Punkt 4; erweitert um ein SFTP-Subsystem für
//! Spec 0020, Abschnitt 6, zweiter Punkt).
//!
//! Kein Docker, keine externe Infrastruktur: `russh` implementiert Client-
//! *und* Server-Seite, ein echter SSH-Server läuft für die Dauer eines
//! Tests einfach als weiterer Tokio-Task im selben Prozess. Das
//! SFTP-Subsystem ist ebenfalls echt (`russh_sftp::server`), gegen ein
//! reales temporäres Verzeichnis auf der lokalen Festplatte (kein
//! In-Memory-Fake) — Integrationstests bekommen so einen echten
//! SFTP-Protokoll-Roundtrip, nicht nur eine simulierte Kontroll-Logik (die
//! deckt bereits `ssh_manager_core::ssh::mock::MockSftpSession` ab).

use std::collections::{HashMap, VecDeque};
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Config, Handler, Msg, Session};
use russh::{Channel, ChannelId, Pty};
use russh_sftp::protocol::{
    Data, File as SftpFile, FileAttributes, Handle as SftpHandle, Name, OpenFlags, Status,
    StatusCode, Version,
};
use tempfile::TempDir;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub const TEST_USERNAME: &str = "testuser";
pub const TEST_PASSWORD: &str = "testpass";

/// Läuft ein `RunningTestServer` und stoppt ihn beim Droppen (best effort —
/// der Shutdown-Kanal wird geschlossen, der Accept-Loop-Task beendet sich
/// dadurch spätestens beim nächsten `select!`-Durchlauf).
pub struct RunningTestServer {
    pub addr: SocketAddr,
    /// Roh-Bytes des Host-Public-Keys, den dieser Server präsentiert — für
    /// Tests, die `HostKeyStore::trust()` gezielt mit dem *richtigen* Key
    /// vorbefüllen wollen, ohne selbst einen Handshake führen zu müssen.
    pub host_public_key: Vec<u8>,
    /// Wurzelverzeichnis des SFTP-Subsystems dieses Servers — alle
    /// SFTP-Pfade (`"/foo"`) werden relativ dazu aufgelöst (Spec 0020,
    /// Abschnitt 6, zweiter Punkt). Tests nutzen dies, um Dateien vor einem
    /// Transfer vorzubelegen bzw. das Ergebnis danach direkt auf der
    /// lokalen Festplatte zu prüfen, ohne selbst SFTP sprechen zu müssen.
    /// Als `TempDir` gehalten (nicht nur `PathBuf`), damit das Verzeichnis
    /// automatisch aufgeräumt wird, sobald der Server (und mit ihm dieser
    /// Wert) gedroppt wird.
    pub sftp_root: TempDir,
    shutdown: Option<oneshot::Sender<()>>,
    accept_task: JoinHandle<()>,
}

impl RunningTestServer {
    /// Startet einen frischen Server auf einem zufälligen, freien Port
    /// (`127.0.0.1:0`) mit frisch generiertem Host-Key. Akzeptiert
    /// Passwort-Auth für `TEST_USERNAME`/`TEST_PASSWORD`, beantwortet
    /// Exec-Requests mit einem vorhersagbaren Echo (`echo:<command>\n`,
    /// Exit-Code 0), PTY/Shell-Requests mit einem einfachen Echo-Loop,
    /// leitet `direct-tcpip`-Kanäle (Jump-Host-Tunneling) an echte
    /// TCP-Ziele weiter, und bedient ein `sftp`-Subsystem gegen ein
    /// temporäres lokales Verzeichnis (`sftp_root`).
    pub async fn start() -> Self {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .expect("Ed25519-Testschlüssel sollte immer erzeugbar sein");
        let host_public_key = key
            .public_key()
            .to_bytes()
            .expect("Public Key sollte immer kodierbar sein");

        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let instance_id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let config = Arc::new(Config {
            keys: vec![key],
            server_id: russh::SshId::Standard(format!("SSH-2.0-testfixture{instance_id}").into()),
            ..Default::default()
        });

        let sftp_root = TempDir::new().expect("temporäres SFTP-Root sollte immer anlegbar sein");
        let sftp_root_path = sftp_root.path().to_path_buf();

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("Bind auf 127.0.0.1:0 sollte immer klappen");
        let addr = listener
            .local_addr()
            .expect("local_addr sollte immer verfügbar sein");

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let accept_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _peer)) = accepted else { break };
                        // Nagle deaktivieren: `russh`s eigener `nodelay`-Config-Schalter
                        // greift nur bei `client::connect()` (das den Socket selbst
                        // aufbaut), nicht wenn wir wie hier einen bereits akzeptierten
                        // `TcpStream` an `run_stream` übergeben.
                        let _ = stream.set_nodelay(true);
                        let config = config.clone();
                        let sftp_root = sftp_root_path.clone();
                        tokio::spawn(async move {
                            let handler = TestHandler {
                                channels: HashMap::new(),
                                sftp_root,
                            };
                            let _ = russh::server::run_stream(config, stream, handler).await;
                        });
                    }
                }
            }
        });

        Self {
            addr,
            host_public_key,
            sftp_root,
            shutdown: Some(shutdown_tx),
            accept_task,
        }
    }
}

impl Drop for RunningTestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        self.accept_task.abort();
    }
}

struct TestHandler {
    /// Offene Session-Channels, indiziert nach `ChannelId` — `russh`
    /// verlangt für ein Subsystem-Request (`subsystem_request`) den
    /// tatsächlichen `Channel<Msg>`, den `channel_open_session` zuvor schon
    /// entgegengenommen hat; ohne diese Zwischenablage wäre er zu diesem
    /// späteren Zeitpunkt nicht mehr erreichbar (nur noch die `ChannelId`).
    channels: HashMap<ChannelId, Channel<Msg>>,
    sftp_root: PathBuf,
}

impl Handler for TestHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == TEST_USERNAME && password == TEST_PASSWORD {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    /// Muss explizit überschrieben werden: der *Default* von
    /// `channel_open_session` nutzt (wie bei `channel_open_direct_tcpip`,
    /// s. u.) das übergebene `ChannelOpenHandle` nicht — ungenutzt gedroppt
    /// bedeutet automatische Ablehnung ("AdministrativelyProhibited"), nicht
    /// automatisches Akzeptieren, wie ein erster (falscher) Blick auf den
    /// Default-Rumpf (`async { Ok(()) }`) nahelegt.
    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Für ein evtl. folgendes `subsystem_request` vorhalten (s.
        // `TestHandler::channels`-Doc-Kommentar) — Exec/PTY/Shell brauchen
        // den gespeicherten Channel selbst nicht (sie arbeiten über
        // `session`+`ChannelId`), das Vorhalten stört sie also nicht.
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data);
        // Spec 0027: simuliert ein nie von selbst endendes Kommando
        // (`journalctl -f`/`tail -f`) — sendet eine erste Zeile, danach
        // absichtlich weder `exit_status_request` noch `eof`/`close`. Der
        // Kanal bleibt offen, bis der *Client* ihn schließt (genau das
        // Verhalten, das `drain_channel_cancellable` testet).
        if command == "never-ending" {
            session.data(channel, b"first line\n".to_vec())?;
            return Ok(());
        }
        session.data(channel, format!("echo:{command}\n").into_bytes())?;
        session.exit_status_request(channel, 0)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Einfache Echo-Shell: alles, was der Client in die PTY tippt, geht
        // unverändert zurück — reicht als vorhersagbares Verhalten für den
        // Integrationstest (kein echtes Betriebssystem-Shell-Backend nötig).
        session.data(channel, data.to_vec())?;
        Ok(())
    }

    /// Jump-Host-Tunneling (Spec 0005 Abschnitt 5): der *default*
    /// `channel_open_direct_tcpip` lehnt still ab (Doc-Kommentar von
    /// `russh::server::Handler`: "Dropping the handle ... automatically
    /// rejects the channel") — dieser Server agiert also selbst wie eine
    /// Bastion und öffnet eine echte TCP-Verbindung zum angeforderten Ziel,
    /// dann werden Bytes in beide Richtungen transparent durchgereicht
    /// (reine Byte-Bridge, kein SSH-Wissen nötig).
    ///
    /// Bekannte Einschränkung (nicht diese Bridge, sondern der darüber
    /// verschachtelte SSH-Handshake): s. `crate::connect`-Doc-Kommentar und
    /// `docs/adr/0008-russh-nested-tunnel-limitation.md`.
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        let target = format!("{host_to_connect}:{port_to_connect}");
        match TcpStream::connect(&target).await {
            Ok(mut tcp) => {
                let _ = tcp.set_nodelay(true);
                reply.accept().await;
                let mut channel_stream = channel.into_stream();
                tokio::spawn(async move {
                    let _ = tokio::io::copy_bidirectional(&mut channel_stream, &mut tcp).await;
                });
            }
            Err(_) => {
                // `reply` einfach droppen -> automatische Ablehnung laut
                // Doc-Kommentar, kein Panic.
                drop(reply);
            }
        }
        Ok(())
    }

    /// Spec 0020, Abschnitt 3/6: `sftp`-Subsystem — bedient von
    /// [`SftpTestHandler`] gegen `self.sftp_root`. `russh_sftp::server::run`
    /// spawnt seine eigene Verarbeitungsschleife und kehrt sofort zurück
    /// (s. dessen Quelltext) — blockiert hier also nicht die Bearbeitung
    /// anderer, gleichzeitig offener Channels derselben Verbindung (z. B.
    /// ein parallel offenes Exec/PTY).
    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            session.channel_failure(channel_id)?;
            return Ok(());
        }
        let Some(channel) = self.channels.remove(&channel_id) else {
            session.channel_failure(channel_id)?;
            return Ok(());
        };
        session.channel_success(channel_id)?;
        let handler = SftpTestHandler {
            root: self.sftp_root.clone(),
            open_files: HashMap::new(),
            open_dirs: HashMap::new(),
            next_handle: 0,
        };
        russh_sftp::server::run(channel.into_stream(), handler).await;
        Ok(())
    }
}

/// Bedient das `SSH_FXP_*`-Protokoll gegen ein echtes lokales Verzeichnis
/// (`root`) — SFTP-Pfade (`"/foo/bar"`) werden relativ dazu aufgelöst.
/// Deckt genau die Operationen ab, die [`ssh_manager_core::ssh::SftpSession`]
/// (Spec 0020, Abschnitt 3) braucht; alle anderen `Handler`-Methoden nutzen
/// die Trait-Defaults (liefern `StatusCode::OpUnsupported`).
struct SftpTestHandler {
    root: PathBuf,
    open_files: HashMap<String, fs::File>,
    open_dirs: HashMap<String, VecDeque<PathBuf>>,
    next_handle: u64,
}

impl SftpTestHandler {
    fn resolve(&self, path: &str) -> PathBuf {
        let trimmed = path.trim_start_matches('/');
        if trimmed.is_empty() {
            self.root.clone()
        } else {
            self.root.join(trimmed)
        }
    }

    fn fresh_handle(&mut self) -> String {
        self.next_handle += 1;
        format!("h{}", self.next_handle)
    }
}

fn ok_status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_string(),
        language_tag: "en-US".to_string(),
    }
}

fn map_io_err(err: std::io::Error) -> StatusCode {
    match err.kind() {
        ErrorKind::NotFound => StatusCode::NoSuchFile,
        ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

impl russh_sftp::server::Handler for SftpTestHandler {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        _version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<SftpHandle, Self::Error> {
        let path = self.resolve(&filename);
        let std_opts: std::fs::OpenOptions = pflags.into();
        let tokio_opts: fs::OpenOptions = std_opts.into();
        let file = tokio_opts.open(&path).await.map_err(map_io_err)?;
        let handle = self.fresh_handle();
        self.open_files.insert(handle.clone(), file);
        Ok(SftpHandle { id, handle })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.open_files.remove(&handle);
        self.open_dirs.remove(&handle);
        Ok(ok_status(id))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let file = self
            .open_files
            .get_mut(&handle)
            .ok_or(StatusCode::Failure)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(map_io_err)?;
        let mut buf = vec![0u8; len as usize];
        let n = file.read(&mut buf).await.map_err(map_io_err)?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        buf.truncate(n);
        Ok(Data { id, data: buf })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let file = self
            .open_files
            .get_mut(&handle)
            .ok_or(StatusCode::Failure)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(map_io_err)?;
        file.write_all(&data).await.map_err(map_io_err)?;
        Ok(ok_status(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<SftpHandle, Self::Error> {
        let dir_path = self.resolve(&path);
        let mut entries = VecDeque::new();
        let mut read_dir = fs::read_dir(&dir_path).await.map_err(map_io_err)?;
        while let Some(entry) = read_dir.next_entry().await.map_err(map_io_err)? {
            entries.push_back(entry.path());
        }
        let handle = self.fresh_handle();
        self.open_dirs.insert(handle.clone(), entries);
        Ok(SftpHandle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let entries = self.open_dirs.get_mut(&handle).ok_or(StatusCode::Failure)?;
        // Alle verbleibenden Einträge in einer Antwort zurückgeben (statt
        // z. B. nur einen); der nächste Aufruf liefert dann `Eof` — genügt
        // für die in Tests realistischen Verzeichnisgrößen und hält diesen
        // Test-Handler einfach.
        if entries.is_empty() {
            return Err(StatusCode::Eof);
        }
        let mut files = Vec::new();
        while let Some(entry_path) = entries.pop_front() {
            let name = entry_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let metadata = fs::metadata(&entry_path).await.map_err(map_io_err)?;
            files.push(SftpFile::new(name, FileAttributes::from(&metadata)));
        }
        Ok(Name { id, files })
    }

    async fn lstat(
        &mut self,
        id: u32,
        path: String,
    ) -> Result<russh_sftp::protocol::Attrs, Self::Error> {
        self.stat(id, path).await
    }

    async fn stat(
        &mut self,
        id: u32,
        path: String,
    ) -> Result<russh_sftp::protocol::Attrs, Self::Error> {
        let resolved = self.resolve(&path);
        let metadata = fs::metadata(&resolved).await.map_err(map_io_err)?;
        Ok(russh_sftp::protocol::Attrs {
            id,
            attrs: FileAttributes::from(&metadata),
        })
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        fs::remove_file(self.resolve(&filename))
            .await
            .map_err(map_io_err)?;
        Ok(ok_status(id))
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        fs::create_dir(self.resolve(&path))
            .await
            .map_err(map_io_err)?;
        Ok(ok_status(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        fs::remove_dir(self.resolve(&path))
            .await
            .map_err(map_io_err)?;
        Ok(ok_status(id))
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        fs::rename(self.resolve(&oldpath), self.resolve(&newpath))
            .await
            .map_err(map_io_err)?;
        Ok(ok_status(id))
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let normalized = if path.is_empty() || path == "." {
            "/".to_string()
        } else {
            path
        };
        Ok(Name {
            id,
            files: vec![SftpFile::dummy(normalized)],
        })
    }
}

/// Nur für Integrationstests, die sich per Pfad direkt gegen das
/// SFTP-Root eines laufenden Testservers verifizieren wollen (Roundtrip-
/// Assertions), ohne selbst SFTP zu sprechen.
pub fn sftp_local_path(server: &RunningTestServer, remote_path: &str) -> PathBuf {
    Path::new(server.sftp_root.path()).join(remote_path.trim_start_matches('/'))
}
