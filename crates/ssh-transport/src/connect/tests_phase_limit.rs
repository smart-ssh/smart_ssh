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
    /// The previous hop refuses to open the tunnel to this hop.
    tunnel_refused: bool,
}

fn timing(handshake_secs: u64, authentication_secs: u64) -> HopTiming {
    HopTiming {
        handshake: Some(Duration::from_secs(handshake_secs)),
        authentication: Some(Duration::from_secs(authentication_secs)),
        unknown_host_key: false,
        tunnel_refused: false,
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
        via_index: usize,
        _via_hop: &Hop,
        hop_index: usize,
        _hop: &Hop,
    ) -> Result<Opened<usize>, SshError> {
        assert_eq!(*via + 1, hop_index, "Tunnel muss über den Vorgänger laufen");
        assert_eq!(via_index + 1, hop_index);
        if self.timings[hop_index].tunnel_refused {
            self.calls.push(format!("refused-by:{via_index}"));
            return Err(SshError::ChannelError(format!(
                "hop {via_index} refused to forward"
            )));
        }
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

/// Issue #1220: a bastion that refuses the tunnel ends the chain; the
/// connector is told which hop is the `via` one, and the target is
/// neither opened nor logged in to.
#[tokio::test(start_paused = true)]
async fn test_refused_tunnel_ends_the_chain_at_the_bastion() {
    let mut fake = FakeHops::new(vec![
        timing(1, 1),
        HopTiming {
            tunnel_refused: true,
            ..timing(1, 1)
        },
        timing(1, 1),
    ]);
    let (result, _) = run(&mut fake).await;
    assert!(matches!(result, Err(SshError::ChannelError(m)) if m.contains("hop 0")));
    assert_eq!(fake.calls, ["open:0", "auth:0", "refused-by:0"]);
}

/// Ohne Hops: sofortiger Fehler, wie vor Issue #97.
#[tokio::test]
async fn test_empty_chain_fails_immediately() {
    let mut fake = FakeHops::new(Vec::new());
    let result = drive_chain(&mut fake, &[], ConnectLimits::DEFAULT).await;
    assert!(matches!(result, Err(SshError::ConnectionFailed(_))));
    assert!(fake.calls.is_empty());
}
