use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use russh::client;
use ssh_manager_core::profiles::CredentialStore;
use ssh_manager_core::ssh::{
    ConnectLog, ConnectStep, ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, KeyFileReader,
    SshError, SshTransport,
};

use crate::auth::{authenticate, AuthSteps};
use crate::error::{map_io_error, map_russh_error, map_transport_error, TransportError};
use crate::handler::{ClientHandler, HopSteps};
use crate::transport::RusshTransport;

/// Ergebnis eines Verbindungsversuchs.
///
/// Weicht von der Signatur in Spec 0005 Abschnitt 4
/// (`connect() -> Result<Box<dyn SshTransport>, SshError>`) bewusst ab:
/// Abschnitt 6 verlangt ausdrücklich, dass ein `Unknown`/`Mismatch`-
/// Host-Key **nicht** stillschweigend im Fehlerfall versteckt wird, sondern
/// die UI-Schicht ihn für einen Bestätigungsdialog nutzen kann. Ein reiner
/// `Result<Box<dyn SshTransport>, SshError>` kann "verbunden" nicht von
/// "wartet auf Bestätigung" unterscheiden, ohne den Host-Key-Fall als
/// Sondervariante von `SshError` zu missbrauchen (der dann fälschlich wie
/// ein harter Fehler behandelt würde). Siehe
/// `docs/adr/0007-connect-outcome-and-arc-host-keys.md`.
pub enum ConnectOutcome {
    Connected(Box<dyn SshTransport>),
    PendingHostKeyConfirmation {
        host: String,
        port: u16,
        raw_key: Vec<u8>,
        decision: HostKeyDecision,
    },
}

/// Baut eine (ggf. über Jump-Hosts verkettete) SSH-Verbindung zu `target`
/// auf (Spec 0005, Abschnitt 4/5).
///
/// `host_keys: Arc<dyn HostKeyStore>` statt `&dyn HostKeyStore` (Spec-
/// Signatur): `russh::client::Handler` verlangt `Self: 'static` (der
/// Handler wird in einen Tokio-Task verschoben, dessen Lebensdauer die des
/// `connect()`-Aufrufs überdauert) — eine geliehene Referenz mit
/// Aufruf-Lebensdauer kann das nicht erfüllen. `credentials` bleibt dagegen
/// eine reine Referenz: Credential-Auflösung passiert synchron in dieser
/// Funktion (bzw. in `crate::auth::authenticate`), nie in einem
/// `russh`-Callback, das den `'static`-Zwang hätte. Siehe
/// `docs/adr/0007-connect-outcome-and-arc-host-keys.md`.
///
/// `&(dyn CredentialStore + Send + Sync)` statt `&dyn CredentialStore`
/// (Spec-Signatur): der Trait selbst verlangt keinen `Send + Sync`-Bound
/// (s. `core::profiles::credentials`), diese Funktion ist aber `async` und
/// hält die Referenz über `.await`-Punkte hinweg — ohne den Bound ist die
/// von `connect()` zurückgegebene Future nicht `Send`, was jeden Aufrufer
/// bricht, der (wie ein Tauri-`#[tauri::command]`) selbst eine `Send`-Future
/// braucht. Nachträglich beim Verdrahten in `crates/app-shell` (Spec 0007,
/// Teil 2) aufgefallen, nicht beim ursprünglichen Schreiben dieser Funktion
/// (die Integrationstests riefen `connect()` bislang nur direkt in
/// `#[tokio::test]`s auf, wo eine `Send`-Future nicht verlangt wird).
///
/// **Bekannte Einschränkung (Jump-Hosts, `remaining_hops`-Zweig):** die
/// Verkettung über `channel_open_direct_tcpip` + `Channel::into_stream()` +
/// `client::connect_stream()` folgt exakt dem in Spec 0005 Abschnitt 5
/// beschriebenen Standard-Tunneling-Verfahren und ist architektonisch
/// korrekt — betrifft nur den Jump-Host-Fall (zweiter und weitere Hops),
/// ein einzelner Hop ist davon nicht betroffen und funktioniert (s.
/// Integrationstests). Gegen `russh` 0.63.1 sendet der über den Tunnel
/// erreichte Ziel-Server (verifiziert per Byte-Level-Tracing direkt auf dem
/// rohen `TcpStream`, nicht nur eine Vermutung) seine eigene
/// SSH-Identifikationszeile ein zweites Mal, unmittelbar vor seiner
/// KEXINIT-Antwort — der Client liest die zweite Kopie fälschlich als
/// 4-Byte-Paketlängen-Präfix und bricht mit "Bad packet size" ab.
/// Ausgeschlossen wurden dabei: TCP-Nagle-Koaleszenz (`nodelay` half
/// nicht), doppelter `channel_open_direct_tcpip`-Aufruf (per Zähler
/// verifiziert: genau 1), doppelter `run_stream`-Aufruf (per Log
/// verifiziert: genau 1) sowie ein zweiter `send_ssh_id`-Aufrufort (es gibt
/// nur einen einzigen in der gesamten `russh`-Quelle). Details und
/// Recherche zu zwei unabhängigen, offenen `russh`-Upstream-Reports mit
/// demselben grundsätzlichen Muster (SSH-über-SSH via
/// `channel_open_direct_tcpip`) in
/// `docs/adr/0008-russh-nested-tunnel-limitation.md`.
///
/// Spec 0076, §4.2: `key_files` ist die äußere Grenze zum Dateisystem für
/// [`ssh_manager_core::profiles::AuthMethod::IdentityFile`] — durchgereicht
/// bis `authenticate`, das je Hop läuft (A-8).
pub async fn connect(
    target: &ConnectionTarget,
    credentials: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_keys: Arc<dyn HostKeyStore>,
) -> Result<ConnectOutcome, SshError> {
    connect_with_log(
        target,
        credentials,
        key_files,
        host_keys,
        &ConnectLog::new(),
    )
    .await
}

/// Zeitgrenzen je Phase des Verbindungsaufbaus (Spec 0069, A3; Issue #97).
///
/// Jeder Hop bekommt **eigene** Grenzen: eine kurze für Verbindung und
/// Handshake (erster Hop: Namensauflösung, TCP, SSH-Handshake samt
/// Host-Key-Prüfung; weitere Hops: Tunnel über den vorherigen Hop und
/// Handshake), eine großzügige für die Anmeldung, damit ein Nutzer einen
/// Hardware-Schlüssel berühren oder eine Agent-Bestätigung abnicken kann.
/// Die Gesamtzeit wächst damit linear mit der Zahl der Hops.
///
/// Die Werte stehen in Spec 0069; [`ConnectLimits::DEFAULT`] bildet sie ab.
/// Das Warten auf eine Host-Key-Entscheidung liegt außerhalb jeder Grenze:
/// eine unbekannte oder geänderte Host-Key beendet den Aufbau sofort mit
/// [`ConnectOutcome::PendingHostKeyConfirmation`], gewartet wird erst beim
/// Aufrufer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectLimits {
    /// Verbindung (DNS + TCP bzw. Tunnel) und SSH-Handshake, je Hop.
    pub handshake: Duration,
    /// Anmeldung, je Hop.
    pub authentication: Duration,
}

impl ConnectLimits {
    /// Spec 0069, A3: 10 s für Verbindung und Handshake, 60 s für die
    /// Anmeldung, jeweils je Hop.
    pub const DEFAULT: ConnectLimits = ConnectLimits {
        handshake: Duration::from_secs(10),
        authentication: Duration::from_secs(60),
    };

    /// Äußere Sicherheitsgrenze für einen ganzen Versuch über `hop_count`
    /// Hops: die Summe aller Phasengrenzen plus eine weitere
    /// Handshake-Grenze als Reserve. Die Phasen laufen nacheinander und sind
    /// jede für sich begrenzt — diese Grenze ist nur ein Netz für den Fall,
    /// dass ein Abschnitt außerhalb einer Phase hängt, und im normalen
    /// Ablauf nicht erreichbar.
    pub fn overall(&self, hop_count: usize) -> Duration {
        let per_hop = self.handshake.saturating_add(self.authentication);
        let hops = u32::try_from(hop_count.max(1)).unwrap_or(u32::MAX);
        per_hop
            .checked_mul(hops)
            .unwrap_or(Duration::MAX)
            .saturating_add(self.handshake)
    }
}

impl Default for ConnectLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Wie [`connect`], zeichnet dabei aber jeden Schritt je Hop in `log` auf
/// (Issue #51): DNS, TCP bzw. Tunnel, Handshake, Host-Key-Prüfung,
/// Anmeldung, fertige Sitzung. Ein Fehler schließt den gerade laufenden
/// Schritt mit dem Code des Fehlers ([`SshError::code`]) — auch ein
/// abgelaufenes Phasenlimit (`SSH_TIMEOUT`).
///
/// Läuft mit [`ConnectLimits::DEFAULT`]; s. [`connect_with_limits`].
///
/// Die Aufzeichnung ändert keine Entscheidung: Host-Key-Prüfung und
/// Anmeldung laufen exakt wie in [`connect`], es wird keine zusätzliche
/// Methode versucht und kein Schlüssel automatisch angenommen.
pub async fn connect_with_log(
    target: &ConnectionTarget,
    credentials: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_keys: Arc<dyn HostKeyStore>,
    log: &ConnectLog,
) -> Result<ConnectOutcome, SshError> {
    connect_with_limits(
        target,
        credentials,
        key_files,
        host_keys,
        log,
        ConnectLimits::DEFAULT,
    )
    .await
}

/// Wie [`connect_with_log`], mit ausdrücklichen Phasengrenzen (Issue #97).
///
/// Läuft eine Phase über ihre Grenze, endet der Versuch mit
/// [`SshError::Timeout`] — nie mit `Connected`, nie mit einer
/// Host-Key-Rückfrage, und ohne dass danach noch eine weitere Phase
/// beginnt. Der laufende Schritt im `log` wird mit `SSH_TIMEOUT`
/// geschlossen. Bricht dagegen ein Aufrufer den Versuch von außen ab (z. B.
/// mit [`connect_with_timeout`]), bleibt der laufende Schritt offen — der
/// Aufrufer schließt ihn mit [`ConnectLog::fail_running`].
pub async fn connect_with_limits(
    target: &ConnectionTarget,
    credentials: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_keys: Arc<dyn HostKeyStore>,
    log: &ConnectLog,
    limits: ConnectLimits,
) -> Result<ConnectOutcome, SshError> {
    let result = connect_inner(target, credentials, key_files, host_keys, log, limits).await;
    if let Err(err) = &result {
        log.fail_running(err.code());
    }
    result
}

async fn connect_inner(
    target: &ConnectionTarget,
    credentials: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    host_keys: Arc<dyn HostKeyStore>,
    log: &ConnectLog,
    limits: ConnectLimits,
) -> Result<ConnectOutcome, SshError> {
    // `nodelay: true` (Nagle deaktiviert): standardmäßig `false` in `russh`.
    // Bei mehrstufigen Verbindungen (Jump-Hosts) kann Nagles Algorithmus
    // dazu führen, dass die ID-Zeile und der direkt folgende KEXINIT-Frame
    // im selben TCP-Segment ankommen/koalesziert werden — für den einfachen
    // Fall unschädlich, im Tunnel-Fall aber ein zusätzlicher Unsicherheits-
    // faktor beim Debuggen von Framing-Problemen.
    let config = Arc::new(client::Config {
        nodelay: true,
        ..Default::default()
    });

    let mut hops = RusshHops {
        config,
        host_keys,
        credentials,
        key_files,
        log,
    };
    let mut sessions = match drive_chain(&mut hops, &target.hops, limits).await? {
        Chain::Ready(sessions) => sessions,
        Chain::HostKeyPending(outcome) => return Ok(outcome),
    };

    let last_index = target.hops.len() - 1;
    let ready = log.start(
        last_index,
        &hop_label(&target.hops[last_index]),
        ConnectStep::SessionReady,
    );
    log.succeed(ready);

    let handle = sessions
        .pop()
        .expect("drive_chain liefert je Hop genau eine Sitzung, mindestens eine");
    Ok(ConnectOutcome::Connected(Box::new(RusshTransport {
        handle,
        _intermediate_hops: sessions,
        max_output_bytes: crate::exec::MAX_STREAM_OUTPUT_BYTES,
    })))
}

/// Ergebnis der Verbindungsphase eines Hops.
enum Opened<S> {
    /// Handshake fertig, Host-Key bekannt — weiter zur Anmeldung.
    Ready(S),
    /// Host-Key unbekannt oder geändert: der Aufbau endet hier, der
    /// Aufrufer fragt den Nutzer (außerhalb jeder Zeitgrenze).
    HostKeyPending(ConnectOutcome),
}

/// Ergebnis der ganzen Kette.
enum Chain<S> {
    /// Eine Sitzung je Hop, in Reihenfolge; die letzte ist das Ziel.
    Ready(Vec<S>),
    HostKeyPending(ConnectOutcome),
}

/// Die drei Phasen eines Hops, getrennt von ihren Zeitgrenzen: die
/// Verkettung und die Grenzen setzt [`drive_chain`], die echte Umsetzung ist
/// [`RusshHops`]. Die Trennung macht die Grenzen mit pausierter Tokio-Uhr
/// testbar, ohne Netzwerk (Issue #97).
///
/// Kein Teil dieses Traits bekommt einen [`HostKeyStore`] zu sehen, den
/// [`drive_chain`] beschreiben könnte: ein Timeout kann strukturell kein
/// `trust()` auslösen.
#[async_trait]
trait HopConnector: Send {
    type Session: Send;

    /// Erster Hop: Namensauflösung, TCP, SSH-Handshake, Host-Key-Prüfung.
    async fn open_first(&mut self, hop: &Hop) -> Result<Opened<Self::Session>, SshError>;

    /// Weiterer Hop: Tunnel über `via`, SSH-Handshake, Host-Key-Prüfung.
    async fn open_next(
        &mut self,
        via: &mut Self::Session,
        hop_index: usize,
        hop: &Hop,
    ) -> Result<Opened<Self::Session>, SshError>;

    /// Anmeldung am gerade geöffneten Hop.
    async fn authenticate(
        &mut self,
        session: &mut Self::Session,
        hop_index: usize,
        hop: &Hop,
    ) -> Result<(), SshError>;
}

/// Spec 0069, A3 (Issue #97): baut die Kette Hop für Hop auf; jede Phase
/// jedes Hops läuft unter ihrer eigenen Grenze aus `limits`. Ein Ablauf
/// liefert `Err(SshError::Timeout)`; danach beginnt keine weitere Phase.
async fn drive_chain<C: HopConnector>(
    connector: &mut C,
    hops: &[Hop],
    limits: ConnectLimits,
) -> Result<Chain<C::Session>, SshError> {
    let Some((first_hop, remaining_hops)) = hops.split_first() else {
        return Err(SshError::ConnectionFailed(
            "ConnectionTarget ohne Hops kann nicht verbunden werden".to_string(),
        ));
    };

    let mut current =
        match connect_with_timeout(connector.open_first(first_hop), limits.handshake).await? {
            Opened::Ready(session) => session,
            Opened::HostKeyPending(outcome) => return Ok(Chain::HostKeyPending(outcome)),
        };
    connect_with_timeout(
        connector.authenticate(&mut current, 0, first_hop),
        limits.authentication,
    )
    .await?;

    let mut sessions = Vec::with_capacity(hops.len());
    for (offset, hop) in remaining_hops.iter().enumerate() {
        let hop_index = offset + 1;
        let next = match connect_with_timeout(
            connector.open_next(&mut current, hop_index, hop),
            limits.handshake,
        )
        .await?
        {
            Opened::Ready(session) => session,
            Opened::HostKeyPending(outcome) => return Ok(Chain::HostKeyPending(outcome)),
        };
        sessions.push(std::mem::replace(&mut current, next));
        connect_with_timeout(
            connector.authenticate(&mut current, hop_index, hop),
            limits.authentication,
        )
        .await?;
    }
    sessions.push(current);
    Ok(Chain::Ready(sessions))
}

/// Die echten Phasen über `russh`.
struct RusshHops<'a> {
    config: Arc<client::Config>,
    host_keys: Arc<dyn HostKeyStore>,
    credentials: &'a (dyn CredentialStore + Send + Sync),
    key_files: &'a (dyn KeyFileReader + Send + Sync),
    log: &'a ConnectLog,
}

#[async_trait]
impl HopConnector for RusshHops<'_> {
    type Session = client::Handle<ClientHandler>;

    async fn open_first(&mut self, hop: &Hop) -> Result<Opened<Self::Session>, SshError> {
        let label = hop_label(hop);
        let socket = match open_tcp(self.log, &label, &hop.host, hop.port).await {
            Ok(socket) => socket,
            // Spec 0069, Teil A3: DNS-Diagnose nur für den ersten Hop, nur
            // im Fehlerfall, nur nachträglich — s. `diagnose_connection_
            // failed_dns`-Doc-Kommentar. Läuft innerhalb der
            // Handshake-Grenze dieses Hops.
            Err(err) => return Err(diagnose_connection_failed_dns(err, &hop.host, hop.port).await),
        };

        let handler = ClientHandler {
            host: hop.host.clone(),
            port: hop.port,
            host_keys: self.host_keys.clone(),
            steps: HopSteps::start(self.log, 0, &label),
        };
        let connect_result = client::connect_stream(self.config.clone(), socket, handler).await;
        match resolve_or_pending(connect_result, &hop.host, hop.port) {
            Ok(Ok(handle)) => Ok(Opened::Ready(handle)),
            Ok(Err(outcome)) => Ok(Opened::HostKeyPending(outcome)),
            Err(err) => Err(diagnose_connection_failed_dns(err, &hop.host, hop.port).await),
        }
    }

    async fn open_next(
        &mut self,
        via: &mut Self::Session,
        hop_index: usize,
        hop: &Hop,
    ) -> Result<Opened<Self::Session>, SshError> {
        let label = hop_label(hop);
        let tunnel_step = self.log.start(
            hop_index,
            &label,
            ConnectStep::TunnelOpen {
                host: hop.host.clone(),
                port: hop.port,
            },
        );
        let tunnel_channel = via
            .channel_open_direct_tcpip(
                hop.host.clone(),
                u32::from(hop.port),
                "127.0.0.1".to_string(),
                0,
            )
            .await
            .map_err(map_russh_error)?;
        self.log.succeed(tunnel_step);
        let stream = tunnel_channel.into_stream();

        let handler = ClientHandler {
            host: hop.host.clone(),
            port: hop.port,
            host_keys: self.host_keys.clone(),
            steps: HopSteps::start(self.log, hop_index, &label),
        };
        let connect_result = client::connect_stream(self.config.clone(), stream, handler).await;
        Ok(
            match resolve_or_pending(connect_result, &hop.host, hop.port)? {
                Ok(handle) => Opened::Ready(handle),
                Err(outcome) => Opened::HostKeyPending(outcome),
            },
        )
    }

    async fn authenticate(
        &mut self,
        session: &mut Self::Session,
        hop_index: usize,
        hop: &Hop,
    ) -> Result<(), SshError> {
        authenticate(
            session,
            hop,
            self.credentials,
            self.key_files,
            &AuthSteps::new(self.log, hop_index, &hop_label(hop)),
        )
        .await
    }
}

/// `user@host:port` — dieselbe Form wie [`ssh_manager_core::ssh::HopLabel`].
fn hop_label(hop: &Hop) -> String {
    format!("{}@{}:{}", hop.username, hop.host, hop.port)
}

/// Issue #51: Namensauflösung und TCP-Verbindung des ersten Hops als zwei
/// eigene, aufgezeichnete Schritte.
///
/// Vorher reichte `connect` `(host, port)` an `russh::client::connect`
/// durch, das intern genau dasselbe tut: `TcpStream::connect` auf die
/// aufgelösten Adressen (jede der Reihe nach, der letzte Fehler gewinnt),
/// danach `set_nodelay` und `connect_stream`. Hier steht derselbe Ablauf
/// ausgeschrieben, damit sich Auflösung und Verbindung getrennt messen
/// lassen. Der Hostname bleibt der Schlüssel der Host-Key-Prüfung (der
/// Handler bekommt `first_hop.host`, nicht die IP).
///
/// Fehler werden abgebildet wie bisher: ein Fehler der Auflösung wird zu
/// [`SshError::HostNotFound`] mit demselben Text, den die nachträgliche
/// DNS-Diagnose (Spec 0069, A3) gebaut hätte; ein Fehler der Verbindung
/// läuft durch dieselbe `io::ErrorKind`-Tabelle wie zuvor.
async fn open_tcp(
    log: &ConnectLog,
    label: &str,
    host: &str,
    port: u16,
) -> Result<tokio::net::TcpStream, SshError> {
    let dns_step = log.start(
        0,
        label,
        ConnectStep::DnsResolution {
            host: host.to_string(),
            port,
            addresses: Vec::new(),
        },
    );
    let addrs: Vec<std::net::SocketAddr> = match tokio::net::lookup_host((host, port)).await {
        Ok(addrs) => addrs.collect(),
        Err(err) => {
            return Err(SshError::HostNotFound(format!(
                "Host '{host}' ist nicht auflösbar (ursprünglicher Fehler: {err})"
            )))
        }
    };
    log.update(dns_step, |step| {
        if let ConnectStep::DnsResolution { addresses, .. } = step {
            *addresses = addrs.iter().map(|a| a.ip().to_string()).collect();
        }
    });
    log.succeed(dns_step);

    let tcp_step = log.start(
        0,
        label,
        ConnectStep::TcpConnect {
            address: None,
            port,
        },
    );
    let socket = tokio::net::TcpStream::connect(addrs.as_slice())
        .await
        .map_err(map_io_error)?;
    // Wie `russh::client::connect` bei `config.nodelay`: ein Fehlschlag
    // ist dort nur eine Warnung und bricht nichts ab.
    let _ = socket.set_nodelay(true);
    let peer = socket.peer_addr().ok().map(|a| a.ip().to_string());
    log.update(tcp_step, |step| {
        if let ConnectStep::TcpConnect { address, .. } = step {
            *address = peer;
        }
    });
    log.succeed(tcp_step);
    Ok(socket)
}

/// Übersetzt das Ergebnis von `client::connect`/`client::connect_stream`:
/// `Ok` bleibt `Ok`, ein Host-Key-Fehler wird zu
/// `Err(ConnectOutcome::PendingHostKeyConfirmation)`, jeder andere Fehler zu
/// `Err(SshError)` (äußeres `Result`, propagiert per `?`).
#[allow(clippy::type_complexity)]
fn resolve_or_pending<H>(
    result: Result<H, TransportError>,
    host: &str,
    port: u16,
) -> Result<Result<H, ConnectOutcome>, SshError> {
    match result {
        Ok(handle) => Ok(Ok(handle)),
        Err(TransportError::HostKey { raw_key, decision }) => {
            Ok(Err(ConnectOutcome::PendingHostKeyConfirmation {
                host: host.to_string(),
                port,
                raw_key,
                decision,
            }))
        }
        Err(other) => Err(map_transport_error(other)),
    }
}

/// Spec 0069, Teil A3, §4.2: DNS wird nachträglich diagnostiziert. Dieser
/// Helfer läuft komplett NACH einem bereits gescheiterten Versuch und
/// ändert nichts an ihm.
///
/// Issue #51 (ADR 0110): Die Namensauflösung ist seitdem ein eigener,
/// aufgezeichneter Schritt in [`open_tcp`] — derselbe Ablauf, den
/// `russh::client::connect` intern hatte, nur ausgeschrieben. Ein Fehler
/// dort wird direkt zu [`SshError::HostNotFound`]; dieser Helfer fängt
/// weiter den Rest (`ConnectionFailed` aus TCP oder Handshake) ab. Der
/// Hostname bleibt Schlüssel der Host-Key-Prüfung, nie die IP. Nur `ConnectionFailed`
/// wird nachdiagnostiziert — die übrigen neuen `SshError`-Varianten (s.
/// `crate::error::map_io_error`) sind bereits präziser zugeordnet und
/// brauchen keine weitere Unterscheidung. Wird ausschließlich für den
/// **ersten** Hop aufgerufen (s. `connect()` oben) — Jump-Hosts ab dem
/// zweiten Hop bleiben wie vor dieser Spec (§2, Nicht-Ziele).
async fn diagnose_connection_failed_dns(err: SshError, host: &str, port: u16) -> SshError {
    let SshError::ConnectionFailed(detail) = &err else {
        return err;
    };
    match tokio::net::lookup_host((host, port)).await {
        Ok(_) => err,
        Err(_) => SshError::HostNotFound(format!(
            "Host '{host}' ist nicht auflösbar (ursprünglicher Fehler: {detail})"
        )),
    }
}

/// Spec 0069, Teil A3: verhindert, dass ein hängender Verbindungsaufbau
/// (TCP-SYN ohne Antwort, eine Gegenstelle, die annimmt, aber nie ein
/// SSH-Banner schickt, …) den Nutzer endlos warten lässt. Generisch über
/// `T`/die Future, damit sie in Tests (11/13) auch eine nie fertig
/// werdende Fake-Future umschließen kann, ohne einen echten
/// Netzwerkaufbau zu brauchen.
///
/// Issue #97: begrenzt jede einzelne Phase in [`drive_chain`] (je Hop
/// Handshake und Anmeldung mit eigenen Grenzen aus [`ConnectLimits`]) und
/// beim Aufrufer (`connect_session`, `test_connection`) den ganzen Versuch
/// mit [`ConnectLimits::overall`] als Sicherheitsnetz. Umschließt bewusst
/// **nur** die übergebene Future — nie das Warten auf eine
/// Host-Key-Entscheidung; das bleibt Sache des Aufrufers (Spec 0069 §5:
/// "Der Connect-Timeout liefert immer einen Fehler, nie `Connected`, nie
/// `trust()`").
pub async fn connect_with_timeout<F, T>(fut: F, timeout: Duration) -> Result<T, SshError>
where
    F: Future<Output = Result<T, SshError>>,
{
    match tokio::time::timeout(timeout, fut).await {
        Ok(result) => result,
        Err(_elapsed) => Err(SshError::Timeout),
    }
}

#[cfg(test)]
mod connect_with_timeout_tests {
    //! Spec 0069, Teil A3, adversarial (ERHÖHT). Der eigentliche
    //! End-to-End-Nachweis gegen einen echten (aber hängenden) SSH-Server
    //! steht in `tests/integration.rs` (Test 13) — hier die reinen,
    //! netzwerkfreien Eigenschaften des Helpers selbst.
    use super::*;

    /// Test 11 (Teil): eine nie fertig werdende Future liefert
    /// `Err(SshError::Timeout)`, nicht Hängen. *Gegenbeweis:* ohne den
    /// `tokio::time::timeout`-Aufruf (z. B. `fut.await` direkt) würde
    /// dieser Test mit `#[tokio::test(start_paused = true)]` nie
    /// terminieren, da die virtuelle Uhr ohne einen `timeout()`, der auf
    /// sie wartet, nicht von selbst voranschreitet — ein hart timeoutender
    /// äußerer Test-Timeout stellt zusätzlich sicher, dass ein
    /// versehentliches "hängt für immer" hier sauber als Fehlschlag
    /// auffällt statt den gesamten Testlauf zu blockieren.
    #[tokio::test(start_paused = true)]
    async fn test_never_finishing_future_yields_ssh_timeout_not_a_hang() {
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            connect_with_timeout(
                std::future::pending::<Result<(), SshError>>(),
                Duration::from_secs(10),
            ),
        )
        .await
        .expect("connect_with_timeout selbst darf nicht hängen");

        assert_eq!(result, Err(SshError::Timeout));
    }

    // `connect_with_timeout`s Signatur (s. oben) nimmt keinen `HostKeyStore`
    // entgegen — sie kann daher strukturell, nicht nur im Testfall, niemals
    // selbst `trust()` aufrufen (0 Aufrufe garantiert durch Konstruktion,
    // nicht durch einen Mock-Zähler zur Laufzeit). Der Aufrufer
    // (`app-shell::commands::connect_session`) übergibt ihr ausschließlich
    // den `ssh_transport::connect(...)`-Aufruf; die Host-Key-Entscheidung
    // liegt vollständig außerhalb dieser Funktion — per Code-Struktur
    // nachprüfbar (wie Test 16 in `tests/integration.rs`), nicht per
    // Laufzeit-Assertion.

    /// Erfolgsfall: eine sofort fertige Future liefert ihr Ergebnis
    /// unverändert durch, ohne auf den Timeout zu warten.
    #[tokio::test]
    async fn test_fast_future_resolves_before_timeout() {
        let result = connect_with_timeout(
            async { Ok::<&'static str, SshError>("connected") },
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(result, Ok("connected"));
    }

    /// Test 12 (Spec 0069, Teil A3, adversarial — spec-reviewer-Fund,
    /// Review dieses Schritts: bislang nur per Code-Struktur belegt, nicht
    /// per Test): modelliert den realen `connect_session`-Fall — die
    /// umschlossene Future liefert schnell ein Ergebnis (dort:
    /// `PendingHostKeyConfirmation`), danach vergeht — AUSSERHALB dieses
    /// Aufrufs, während der eigentlichen Host-Key-Wartezeit
    /// (`PENDING_ACTION_CONFIRM_TIMEOUT`, Spec 0068 Teil 5b) — mehr Zeit
    /// als `SSH_CONNECT_TIMEOUT`. Der `.await` oben ist zu diesem
    /// Zeitpunkt bereits vollständig abgeschlossen; es gibt keinen
    /// zweiten Poll-Punkt, an dem der 10-Sekunden-Timeout erneut greifen
    /// könnte. Deckt NICHT die Verdrahtung in `commands.rs` selbst ab (das
    /// ist laut Spec 0069 §2, Nicht-Ziele, bewusst nicht mockbar) — dafür
    /// bleibt der Code-Struktur-Nachweis (`connect_session`s Kommentar,
    /// spec-reviewer-Review) maßgeblich; dieser Test sichert nur, dass
    /// `connect_with_timeout` selbst keine Zeit über sein eigenes
    /// `.await` hinaus "mitzählt".
    #[tokio::test(start_paused = true)]
    async fn test_timeout_does_not_apply_to_time_after_it_already_returned() {
        let result = connect_with_timeout(
            async { Ok::<&'static str, SshError>("pending-host-key-confirmation") },
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(result, Ok("pending-host-key-confirmation"));

        // Simuliert die potenziell sehr lange Host-Key-Wartezeit, die in
        // `connect_session` erst NACH diesem (bereits abgeschlossenen)
        // Aufruf beginnt.
        tokio::time::advance(Duration::from_secs(3601)).await;
    }
}

#[cfg(test)]
mod phase_limit_tests {
    //! Issue #97 (Spec 0069, A3): je Hop eigene Grenzen für Handshake und
    //! Anmeldung. Netzwerkfrei über einen [`HopConnector`]-Fake mit
    //! pausierter Tokio-Uhr — die Verzögerungen sind virtuell, kein Test
    //! wartet wirklich. Jeder Test ist zusätzlich mit einem äußeren
    //! `tokio::time::timeout` abgesichert, damit ein Regressionsfehler
    //! (fehlende Grenze → nie fertig) als Fehlschlag auffällt statt den
    //! Testlauf zu blockieren.
    use super::*;
    use ssh_manager_core::profiles::{AuthMethod, CredentialRef};
    use tokio::time::Instant;

    /// Dauer einer Phase: `None` = wird nie fertig.
    #[derive(Clone, Copy)]
    struct HopTiming {
        handshake: Option<Duration>,
        authentication: Option<Duration>,
        unknown_host_key: bool,
    }

    fn timing(handshake_secs: u64, authentication_secs: u64) -> HopTiming {
        HopTiming {
            handshake: Some(Duration::from_secs(handshake_secs)),
            authentication: Some(Duration::from_secs(authentication_secs)),
            unknown_host_key: false,
        }
    }

    struct FakeHops {
        timings: Vec<HopTiming>,
        calls: Vec<String>,
    }

    impl FakeHops {
        fn new(timings: Vec<HopTiming>) -> Self {
            Self {
                timings,
                calls: Vec::new(),
            }
        }

        async fn open(&mut self, hop_index: usize) -> Result<Opened<usize>, SshError> {
            self.calls.push(format!("open:{hop_index}"));
            let t = self.timings[hop_index];
            wait(t.handshake).await;
            if t.unknown_host_key {
                return Ok(Opened::HostKeyPending(
                    ConnectOutcome::PendingHostKeyConfirmation {
                        host: format!("hop{hop_index}.invalid"),
                        port: 22,
                        raw_key: b"raw-key".to_vec(),
                        decision: HostKeyDecision::Unknown {
                            fingerprint: "SHA256:fake".to_string(),
                        },
                    },
                ));
            }
            Ok(Opened::Ready(hop_index))
        }
    }

    async fn wait(duration: Option<Duration>) {
        match duration {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending::<()>().await,
        }
    }

    #[async_trait]
    impl HopConnector for FakeHops {
        type Session = usize;

        async fn open_first(&mut self, _hop: &Hop) -> Result<Opened<usize>, SshError> {
            self.open(0).await
        }

        async fn open_next(
            &mut self,
            via: &mut usize,
            hop_index: usize,
            _hop: &Hop,
        ) -> Result<Opened<usize>, SshError> {
            assert_eq!(*via + 1, hop_index, "Tunnel muss über den Vorgänger laufen");
            self.open(hop_index).await
        }

        async fn authenticate(
            &mut self,
            session: &mut usize,
            hop_index: usize,
            _hop: &Hop,
        ) -> Result<(), SshError> {
            assert_eq!(*session, hop_index);
            self.calls.push(format!("auth:{hop_index}"));
            wait(self.timings[hop_index].authentication).await;
            Ok(())
        }
    }

    fn hops(count: usize) -> Vec<Hop> {
        (0..count)
            .map(|i| Hop {
                host: format!("hop{i}.invalid"),
                port: 22,
                username: "deploy".to_string(),
                auth: AuthMethod::Password {
                    credential_ref: CredentialRef::new("test:password"),
                },
            })
            .collect()
    }

    /// Ergebnis und verstrichene (virtuelle) Zeit eines Aufbaus mit den
    /// Grenzen aus der Spec.
    async fn run(fake: &mut FakeHops) -> (Result<Chain<usize>, SshError>, Duration) {
        let hop_list = hops(fake.timings.len());
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(24 * 3600),
            drive_chain(fake, &hop_list, ConnectLimits::DEFAULT),
        )
        .await
        .expect("drive_chain darf mit Phasengrenzen nie hängen");
        (result, start.elapsed())
    }

    fn ready_sessions(result: Result<Chain<usize>, SshError>) -> Vec<usize> {
        match result {
            Ok(Chain::Ready(sessions)) => sessions,
            Ok(Chain::HostKeyPending(_)) => panic!("erwartet Ready, bekam HostKeyPending"),
            Err(err) => panic!("erwartet Ready, bekam {err:?}"),
        }
    }

    fn assert_timeout(result: Result<Chain<usize>, SshError>) {
        match result {
            Err(SshError::Timeout) => {}
            Err(other) => panic!("erwartet SshError::Timeout, bekam {other:?}"),
            Ok(Chain::Ready(_)) => panic!("ein Timeout darf nie als verbunden gelten"),
            Ok(Chain::HostKeyPending(_)) => {
                panic!("ein Timeout darf nie in eine Host-Key-Rückfrage münden")
            }
        }
    }

    #[test]
    fn test_default_limits_are_the_spec_values() {
        assert_eq!(ConnectLimits::DEFAULT.handshake, Duration::from_secs(10));
        assert_eq!(
            ConnectLimits::DEFAULT.authentication,
            Duration::from_secs(60)
        );
        assert_eq!(ConnectLimits::default(), ConnectLimits::DEFAULT);
    }

    /// Das Sicherheitsnetz wächst mit der Hop-Zahl und liegt über der Summe
    /// aller Phasengrenzen — es kann im normalen Ablauf nicht vor einer
    /// Phasengrenze greifen.
    #[test]
    fn test_overall_limit_exceeds_the_sum_of_all_phase_limits() {
        let limits = ConnectLimits::DEFAULT;
        assert_eq!(limits.overall(1), Duration::from_secs(80));
        assert_eq!(limits.overall(3), Duration::from_secs(220));
        for hop_count in 1..=8u32 {
            let sum = (limits.handshake + limits.authentication) * hop_count;
            assert!(limits.overall(hop_count as usize) > sum);
        }
        // Ohne Hops scheitert der Aufbau sofort; die Grenze bleibt sinnvoll.
        assert_eq!(limits.overall(0), limits.overall(1));
    }

    /// AC 1 (netzwerkfrei; der echte hängende Server steht in
    /// `tests/integration.rs`): ein Handshake, der nie fertig wird, endet
    /// nach genau der Handshake-Grenze mit `SSH_TIMEOUT`, ohne dass die
    /// Anmeldung beginnt.
    #[tokio::test(start_paused = true)]
    async fn test_hanging_handshake_fails_after_the_handshake_limit() {
        let mut fake = FakeHops::new(vec![HopTiming {
            handshake: None,
            ..timing(0, 0)
        }]);
        let (result, elapsed) = run(&mut fake).await;
        assert_timeout(result);
        assert_eq!(elapsed, ConnectLimits::DEFAULT.handshake);
        assert_eq!(fake.calls, ["open:0"]);
    }

    /// AC 2: eine Anmeldung, die länger dauert als die Handshake-Grenze,
    /// aber kürzer als die Anmeldegrenze (Touch-Schlüssel, Agent mit
    /// Bestätigung), bricht nicht ab.
    ///
    /// *Gegenbeweis:* mit einer gemeinsamen 10-s-Grenze über alles (Stand
    /// vor Issue #97) oder der Handshake-Grenze für die Anmeldung scheitert
    /// dieser Test mit `SSH_TIMEOUT`.
    #[tokio::test(start_paused = true)]
    async fn test_slow_authentication_within_its_limit_connects() {
        let mut fake = FakeHops::new(vec![timing(2, 30)]);
        let (result, elapsed) = run(&mut fake).await;
        assert_eq!(ready_sessions(result), [0]);
        assert_eq!(elapsed, Duration::from_secs(32));
        assert_eq!(fake.calls, ["open:0", "auth:0"]);
    }

    /// AC 3: eine Anmeldung über der Anmeldegrenze endet mit `SSH_TIMEOUT`.
    ///
    /// *Gegenbeweis:* ohne Grenze um die Anmeldung wird dieser Test nie
    /// fertig und scheitert am äußeren Timeout.
    #[tokio::test(start_paused = true)]
    async fn test_authentication_beyond_its_limit_fails_with_timeout() {
        let mut fake = FakeHops::new(vec![timing(2, 61)]);
        let (result, elapsed) = run(&mut fake).await;
        assert_timeout(result);
        assert_eq!(
            elapsed,
            Duration::from_secs(2) + ConnectLimits::DEFAULT.authentication
        );

        let mut never = FakeHops::new(vec![HopTiming {
            authentication: None,
            ..timing(2, 0)
        }]);
        let (result, _) = run(&mut never).await;
        assert_timeout(result);
    }

    /// AC 4: in einer Kette aus zwei Hops bekommt jeder Hop eigene Grenzen.
    /// Jeder Hop braucht hier knapp unter seinen Grenzen (9 s + 59 s); die
    /// Gesamtzeit von 136 s liegt weit über den Grenzen eines einzelnen
    /// Hops (70 s) — die Kette verbindet trotzdem.
    #[tokio::test(start_paused = true)]
    async fn test_two_hop_chain_gets_limits_per_hop() {
        let mut fake = FakeHops::new(vec![timing(9, 59), timing(9, 59)]);
        let (result, elapsed) = run(&mut fake).await;
        assert_eq!(ready_sessions(result), [0, 1]);
        assert_eq!(elapsed, Duration::from_secs(136));
        let single_hop = ConnectLimits::DEFAULT.handshake + ConnectLimits::DEFAULT.authentication;
        assert!(elapsed > single_hop);
        assert_eq!(fake.calls, ["open:0", "auth:0", "open:1", "auth:1"]);
    }

    /// Die Grenzen gelten auch am zweiten Hop: ein hängender Tunnel bzw.
    /// Handshake dort endet nach der Handshake-Grenze, eine zu lange
    /// Anmeldung dort nach der Anmeldegrenze — jeweils `SSH_TIMEOUT`, und es
    /// beginnt keine weitere Phase.
    #[tokio::test(start_paused = true)]
    async fn test_second_hop_phases_are_limited_too() {
        let mut hanging_tunnel = FakeHops::new(vec![
            timing(1, 1),
            HopTiming {
                handshake: None,
                ..timing(0, 0)
            },
        ]);
        let (result, elapsed) = run(&mut hanging_tunnel).await;
        assert_timeout(result);
        assert_eq!(elapsed, Duration::from_secs(12));
        assert_eq!(hanging_tunnel.calls, ["open:0", "auth:0", "open:1"]);

        let mut slow_auth = FakeHops::new(vec![timing(1, 1), timing(1, 61)]);
        let (result, elapsed) = run(&mut slow_auth).await;
        assert_timeout(result);
        assert_eq!(elapsed, Duration::from_secs(63));
    }

    /// Ein Timeout am ersten Hop beendet die Kette: kein Tunnel, keine
    /// weitere Anmeldung.
    #[tokio::test(start_paused = true)]
    async fn test_timeout_stops_the_chain() {
        let mut fake = FakeHops::new(vec![
            HopTiming {
                authentication: None,
                ..timing(1, 0)
            },
            timing(1, 1),
        ]);
        let (result, _) = run(&mut fake).await;
        assert_timeout(result);
        assert_eq!(fake.calls, ["open:0", "auth:0"]);
    }

    /// Eine unbekannte Host-Key am zweiten Hop beendet den Aufbau mit der
    /// Rückfrage — nach dieser Rückgabe läuft keine Grenze weiter, auch wenn
    /// der Nutzer danach lange überlegt.
    #[tokio::test(start_paused = true)]
    async fn test_host_key_question_ends_the_attempt_outside_every_limit() {
        let mut fake = FakeHops::new(vec![
            timing(1, 1),
            HopTiming {
                unknown_host_key: true,
                ..timing(1, 0)
            },
        ]);
        let (result, _) = run(&mut fake).await;
        assert!(matches!(
            result,
            Ok(Chain::HostKeyPending(
                ConnectOutcome::PendingHostKeyConfirmation { .. }
            ))
        ));
        assert_eq!(fake.calls, ["open:0", "auth:0", "open:1"]);
        // Simuliert die Bedenkzeit des Nutzers nach der Rückgabe.
        tokio::time::advance(Duration::from_secs(3601)).await;
    }

    /// Ohne Hops: sofortiger Fehler, wie vor Issue #97.
    #[tokio::test]
    async fn test_empty_chain_fails_immediately() {
        let mut fake = FakeHops::new(Vec::new());
        let result = drive_chain(&mut fake, &[], ConnectLimits::DEFAULT).await;
        assert!(matches!(result, Err(SshError::ConnectionFailed(_))));
        assert!(fake.calls.is_empty());
    }
}
