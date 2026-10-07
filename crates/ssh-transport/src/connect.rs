use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

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
    connect_with_log(target, credentials, key_files, host_keys, &ConnectLog::new()).await
}

/// Wie [`connect`], zeichnet dabei aber jeden Schritt je Hop in `log` auf
/// (Issue #51): DNS, TCP bzw. Tunnel, Handshake, Host-Key-Prüfung,
/// Anmeldung, fertige Sitzung. Ein Fehler schließt den gerade laufenden
/// Schritt mit dem Code des Fehlers ([`SshError::code`]).
///
/// Bricht ein Timeout den Versuch von außen ab, bleibt der laufende Schritt
/// offen — der Aufrufer schließt ihn mit [`ConnectLog::fail_running`].
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
    let result = connect_inner(target, credentials, key_files, host_keys, log).await;
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

    let first_label = hop_label(first_hop);
    let socket = match open_tcp(log, &first_label, &first_hop.host, first_hop.port).await {
        Ok(socket) => socket,
        // Spec 0069, Teil A3: DNS-Diagnose nur für den ersten Hop, nur
        // im Fehlerfall, nur nachträglich — s. `diagnose_connection_
        // failed_dns`-Doc-Kommentar.
        Err(err) => {
            return Err(
                diagnose_connection_failed_dns(err, &first_hop.host, first_hop.port).await,
            )
        }
    };

    let handler = ClientHandler {
        host: first_hop.host.clone(),
        port: first_hop.port,
        host_keys: host_keys.clone(),
        steps: HopSteps::start(log, 0, &first_label),
    };
    let connect_result = client::connect_stream(config.clone(), socket, handler).await;
    let mut current_handle =
        match resolve_or_pending(connect_result, &first_hop.host, first_hop.port) {
            Ok(Ok(handle)) => handle,
            Ok(Err(outcome)) => return Ok(outcome),
            Err(err) => {
                return Err(
                    diagnose_connection_failed_dns(err, &first_hop.host, first_hop.port).await,
                )
            }
        };

    authenticate(
        &mut current_handle,
        first_hop,
        credentials,
        key_files,
        &AuthSteps::new(log, 0, &first_label),
    )
    .await?;

    let mut intermediate_hops = Vec::new();

    for (offset, hop) in remaining_hops.iter().enumerate() {
        let hop_index = offset + 1;
        let label = hop_label(hop);
        let tunnel_step = log.start(
            hop_index,
            &label,
            ConnectStep::TunnelOpen {
                host: hop.host.clone(),
                port: hop.port,
            },
        );
        let tunnel_channel = current_handle
            .channel_open_direct_tcpip(
                hop.host.clone(),
                u32::from(hop.port),
                "127.0.0.1".to_string(),
                0,
            )
            .await
            .map_err(map_russh_error)?;
        log.succeed(tunnel_step);
        let stream = tunnel_channel.into_stream();

        let handler = ClientHandler {
            host: hop.host.clone(),
            port: hop.port,
            host_keys: host_keys.clone(),
            steps: HopSteps::start(log, hop_index, &label),
        };
        let connect_result = client::connect_stream(config.clone(), stream, handler).await;
        let next_handle = match resolve_or_pending(connect_result, &hop.host, hop.port)? {
            Ok(handle) => handle,
            Err(outcome) => return Ok(outcome),
        };

        let previous_handle = std::mem::replace(&mut current_handle, next_handle);
        intermediate_hops.push(previous_handle);

        authenticate(
            &mut current_handle,
            hop,
            credentials,
            key_files,
            &AuthSteps::new(log, hop_index, &label),
        )
        .await?;
    }

    let last_index = target.hops.len() - 1;
    let ready = log.start(
        last_index,
        &hop_label(&target.hops[last_index]),
        ConnectStep::SessionReady,
    );
    log.succeed(ready);

    Ok(ConnectOutcome::Connected(Box::new(RusshTransport {
        handle: current_handle,
        _intermediate_hops: intermediate_hops,
        max_output_bytes: crate::exec::MAX_STREAM_OUTPUT_BYTES,
    })))
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

/// Spec 0069, Teil A3, §4.2: DNS wird **nachträglich** diagnostiziert, nicht
/// vorab aufgelöst. Der eigentliche Verbindungsversuch oben (`client::
/// connect`) reicht `first_hop.host` unverändert an `russh` durch — dieser
/// Helfer läuft komplett NACH einem bereits gescheiterten Versuch und
/// ändert nichts an ihm (keine Vorab-Auflösung, kein Ersetzen des
/// Hostnamens durch eine IP, der Erfolgs- und der Host-Key-Pfad bleiben
/// dadurch byte-gleich zum bisherigen Verhalten). Nur `ConnectionFailed`
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
/// SSH-Banner schickt, …) den Nutzer endlos warten lässt — vorher rief
/// `connect_session` `ssh_transport::connect` ohne jeden Timeout auf (nur
/// `test_connection` hatte eine eigene 10-Sekunden-Grenze). Generisch über
/// `T`/die Future, damit sie in Tests (11/13) auch eine nie fertig
/// werdende Fake-Future umschließen kann, ohne einen echten
/// Netzwerkaufbau zu brauchen. Umschließt bewusst **nur** die übergebene
/// Future — nie das Warten auf eine Host-Key-Entscheidung; das bleibt
/// Sache des Aufrufers (`app-shell::commands::connect_session`, der jeden
/// `ssh_transport::connect`-Aufruf einzeln hiermit umschließt, aber NICHT
/// den Host-Key-Wartezyklus, s. dortiger Kommentar und Spec 0069 §5:
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
