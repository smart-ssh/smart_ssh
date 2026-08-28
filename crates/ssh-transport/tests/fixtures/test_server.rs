//! In-process `russh`-Test-Server (Spec 0005, Abschnitt 8, zweiter Punkt;
//! Aufgabenstellung Teil 2, Punkt 4).
//!
//! Kein Docker, keine externe Infrastruktur: `russh` implementiert Client-
//! *und* Server-Seite, ein echter SSH-Server läuft für die Dauer eines
//! Tests einfach als weiterer Tokio-Task im selben Prozess.

use std::net::SocketAddr;
use std::sync::Arc;

use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Config, Handler, Msg, Session};
use russh::{Channel, ChannelId, Pty};
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
    shutdown: Option<oneshot::Sender<()>>,
    accept_task: JoinHandle<()>,
}

impl RunningTestServer {
    /// Startet einen frischen Server auf einem zufälligen, freien Port
    /// (`127.0.0.1:0`) mit frisch generiertem Host-Key. Akzeptiert
    /// Passwort-Auth für `TEST_USERNAME`/`TEST_PASSWORD`, beantwortet
    /// Exec-Requests mit einem vorhersagbaren Echo (`echo:<command>\n`,
    /// Exit-Code 0), PTY/Shell-Requests mit einem einfachen Echo-Loop, und
    /// leitet `direct-tcpip`-Kanäle (Jump-Host-Tunneling) an echte
    /// TCP-Ziele weiter.
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
                        tokio::spawn(async move {
                            let _ = russh::server::run_stream(config, stream, TestHandler).await;
                        });
                    }
                }
            }
        });

        Self {
            addr,
            host_public_key,
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

struct TestHandler;

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
        _channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
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
}
