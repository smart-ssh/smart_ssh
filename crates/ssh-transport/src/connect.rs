use std::sync::Arc;

use russh::client;
use ssh_manager_core::profiles::CredentialStore;
use ssh_manager_core::ssh::{
    ConnectionTarget, HostKeyDecision, HostKeyStore, SshError, SshTransport,
};

use crate::auth::authenticate;
use crate::error::{map_russh_error, map_transport_error, TransportError};
use crate::handler::ClientHandler;
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
pub async fn connect(
    target: &ConnectionTarget,
    credentials: &dyn CredentialStore,
    host_keys: Arc<dyn HostKeyStore>,
) -> Result<ConnectOutcome, SshError> {
    let Some((first_hop, remaining_hops)) = target.hops.split_first() else {
        return Err(SshError::ConnectionFailed(
            "ConnectionTarget ohne Hops kann nicht verbunden werden".to_string(),
        ));
    };

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

    let handler = ClientHandler {
        host: first_hop.host.clone(),
        port: first_hop.port,
        host_keys: host_keys.clone(),
    };
    let connect_result = client::connect(
        config.clone(),
        (first_hop.host.as_str(), first_hop.port),
        handler,
    )
    .await;
    let mut current_handle =
        match resolve_or_pending(connect_result, &first_hop.host, first_hop.port)? {
            Ok(handle) => handle,
            Err(outcome) => return Ok(outcome),
        };

    authenticate(&mut current_handle, first_hop, credentials).await?;

    let mut intermediate_hops = Vec::new();

    for hop in remaining_hops {
        let tunnel_channel = current_handle
            .channel_open_direct_tcpip(
                hop.host.clone(),
                u32::from(hop.port),
                "127.0.0.1".to_string(),
                0,
            )
            .await
            .map_err(map_russh_error)?;
        let stream = tunnel_channel.into_stream();

        let handler = ClientHandler {
            host: hop.host.clone(),
            port: hop.port,
            host_keys: host_keys.clone(),
        };
        let connect_result = client::connect_stream(config.clone(), stream, handler).await;
        let next_handle = match resolve_or_pending(connect_result, &hop.host, hop.port)? {
            Ok(handle) => handle,
            Err(outcome) => return Ok(outcome),
        };

        let previous_handle = std::mem::replace(&mut current_handle, next_handle);
        intermediate_hops.push(previous_handle);

        authenticate(&mut current_handle, hop, credentials).await?;
    }

    Ok(ConnectOutcome::Connected(Box::new(RusshTransport {
        handle: current_handle,
        _intermediate_hops: intermediate_hops,
    })))
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
