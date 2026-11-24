//! Host key trust (Spec 0005 §6, §10) against real OpenSSH daemons: unknown key
//! pause and trust, changed key, fingerprints equal to `ssh-keygen -lf`, key type
//! switch, trust per port and host certificates. Scenarios: `hostkey-*`. Run
//! through `tests/sshd/run.sh`.
#![expect(clippy::unwrap_used, reason = "integration test")]

mod sshd_support;

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Mutex};

use russh::keys::ssh_key::{HashAlg, PublicKey};
use ssh_manager_core::ssh::{
    ConnectionTarget, HostKeyDecision, HostKeyStore, NoPrompter, SshError,
};
use ssh_transport::ConnectOutcome;
use sshd_support::{
    daemon_log, hop, restart, scenario_container, scenario_port, step, Creds, NoKeyFiles,
};

const HOST: &str = "127.0.0.1";

fn algorithm_of(raw: &[u8]) -> String {
    // SSH wire format: u32 length + algorithm name.
    let len = u32::from_be_bytes(raw[..4].try_into().unwrap()) as usize;
    String::from_utf8(raw[4..4 + len].to_vec()).unwrap()
}

fn fingerprint_of(raw: &[u8]) -> String {
    PublicKey::from_bytes(raw)
        .unwrap()
        .fingerprint(HashAlg::Sha256)
        .to_string()
}

/// Per-algorithm in-memory store like the app's (`host_key_store.rs`): the key
/// is looked up by (host, port, algorithm); fingerprints are `SHA256:<base64>`.
#[derive(Default)]
struct Store {
    known: Mutex<HashMap<(String, u16, String), Vec<u8>>>,
    trusted: Mutex<Vec<(String, u16, String)>>,
}
impl Store {
    fn stored(&self, port: u16, algo: &str) -> Option<Vec<u8>> {
        self.known
            .lock()
            .unwrap()
            .get(&(HOST.to_string(), port, algo.to_string()))
            .cloned()
    }
}
impl HostKeyStore for Store {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        let algo = algorithm_of(key);
        match self
            .known
            .lock()
            .unwrap()
            .get(&(host.to_string(), port, algo))
        {
            None => HostKeyDecision::Unknown {
                fingerprint: fingerprint_of(key),
            },
            Some(s) if s.as_slice() == key => HostKeyDecision::Trusted,
            Some(s) => HostKeyDecision::Mismatch {
                expected_fingerprint: fingerprint_of(s),
                actual_fingerprint: fingerprint_of(key),
            },
        }
    }
    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError> {
        let algo = algorithm_of(key);
        self.trusted
            .lock()
            .unwrap()
            .push((host.to_string(), port, algo.clone()));
        self.known
            .lock()
            .unwrap()
            .insert((host.to_string(), port, algo), key.to_vec());
        Ok(())
    }
}

async fn connect(store: &Arc<Store>, port: u16) -> ConnectOutcome {
    let target = ConnectionTarget {
        hops: vec![hop(HOST, port)],
    };
    let keys: Arc<dyn HostKeyStore> = store.clone();
    step(
        "connect()",
        ssh_transport::connect(&target, &Creds, &NoKeyFiles, keys, &NoPrompter),
    )
    .await
    .unwrap_or_else(|e| panic!("connect() failed instead of pausing or connecting: {e:?}"))
}

/// Expects a pause with an unknown key; returns `(raw_key, fingerprint)`.
fn expect_unknown(outcome: ConnectOutcome, port: u16) -> (Vec<u8>, String) {
    match outcome {
        ConnectOutcome::PendingHostKeyConfirmation {
            host,
            port: p,
            raw_key,
            decision: HostKeyDecision::Unknown { fingerprint },
        } => {
            assert_eq!((host.as_str(), p), (HOST, port));
            (raw_key, fingerprint)
        }
        ConnectOutcome::PendingHostKeyConfirmation { decision, .. } => {
            panic!("expected Unknown, got {decision:?}")
        }
        ConnectOutcome::Connected(_) => panic!("connected without confirming the host key"),
    }
}

/// Restarts the container and waits until sshd shows its banner again.
/// Docker assigns a new host port on restart (the scenario publishes a random
/// one), so the new port is read back and the store entries of `old_port` are
/// moved to it: the test then sees "the same server" under the new port.
/// Returns the new port.
async fn restart_and_wait(scenario: &str, store: &Store, old_port: u16) -> u16 {
    use tokio::io::AsyncReadExt;
    restart(scenario);
    let out = Command::new("docker")
        .args(["port", &scenario_container(scenario), "22/tcp"])
        .output()
        .unwrap();
    assert!(out.status.success(), "docker port failed");
    let port: u16 = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .next()
        .and_then(|l| l.rsplit(':').next())
        .and_then(|p| p.trim().parse().ok())
        .expect("published port after restart");
    {
        let mut known = store.known.lock().unwrap();
        let moved: Vec<_> = known
            .keys()
            .filter(|(_, p, _)| *p == old_port)
            .cloned()
            .collect();
        for k in moved {
            let v = known.remove(&k).unwrap();
            known.insert((k.0, port, k.2), v);
        }
    }
    step("sshd banner after restart", async {
        loop {
            if let Ok(mut s) = tokio::net::TcpStream::connect((HOST, port)).await {
                let mut buf = [0u8; 4];
                if s.read_exact(&mut buf).await.is_ok() && &buf == b"SSH-" {
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    })
    .await;
    port
}

fn container_exec(scenario: &str, args: &[&str]) -> String {
    let out = Command::new("docker")
        .arg("exec")
        .arg(scenario_container(scenario))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "docker exec {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// `SHA256:…` of a public key file inside the container, from `ssh-keygen -lf`
/// (output: `<bits> SHA256:<base64> <comment> (<TYPE>)`).
fn server_fingerprint(scenario: &str, pub_file: &str) -> String {
    let out = container_exec(scenario, &["ssh-keygen", "-lf", pub_file]);
    out.split_whitespace().nth(1).unwrap().to_string()
}

/// Starts after the end of the daemon log (for "nothing happened since").
fn log_since(scenario: &str, offset: usize) -> String {
    daemon_log(scenario)[offset..].to_string()
}

fn assert_no_auth(log: &str) {
    for marker in ["Accepted", "Failed", "Invalid user", "authenticating user"] {
        assert!(
            !log.contains(marker),
            "daemon saw an authentication attempt ({marker}):\n{log}"
        );
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_key_pauses_then_trusted_key_connects_without_pause() {
    let scenario = "hostkey-plain";
    let port = scenario_port(scenario);
    let store = Arc::new(Store::default());
    let log_start = daemon_log(scenario).len();

    let (raw, _) = expect_unknown(connect(&store, port).await, port);
    // Credentials never go to a server whose key is not confirmed.
    assert_no_auth(&log_since(scenario, log_start));
    assert!(
        store.trusted.lock().unwrap().is_empty(),
        "stored without asking"
    );

    store.trust(HOST, port, &raw).unwrap();
    assert_eq!(store.stored(port, &algorithm_of(&raw)), Some(raw));

    let mut t = match connect(&store, port).await {
        ConnectOutcome::Connected(t) => t,
        ConnectOutcome::PendingHostKeyConfirmation { decision, .. } => {
            panic!("second connect paused again: {decision:?}")
        }
    };
    let out = step("execute", t.execute("echo ok")).await.unwrap();
    assert_eq!(out.stdout, b"ok\n");
    assert!(daemon_log(scenario).contains("Accepted password for testuser"));
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_key_stops_with_mismatch_and_no_authentication() {
    let scenario = "hostkey-changed";
    let port = scenario_port(scenario);
    let store = Arc::new(Store::default());

    let (raw, _) = expect_unknown(connect(&store, port).await, port);
    store.trust(HOST, port, &raw).unwrap();
    assert!(matches!(
        connect(&store, port).await,
        ConnectOutcome::Connected(_)
    ));
    // Proves that the daemon log shows authentications at all.
    assert!(daemon_log(scenario).contains("Accepted password for testuser"));

    let port = restart_and_wait(scenario, &store, port).await; // setup.sh generates new host keys on every start
    let log_start = daemon_log(scenario).len();

    match connect(&store, port).await {
        ConnectOutcome::PendingHostKeyConfirmation {
            raw_key,
            decision:
                HostKeyDecision::Mismatch {
                    expected_fingerprint,
                    actual_fingerprint,
                },
            ..
        } => {
            assert_eq!(expected_fingerprint, fingerprint_of(&raw));
            assert_eq!(actual_fingerprint, fingerprint_of(&raw_key));
            assert_ne!(expected_fingerprint, actual_fingerprint);
        }
        ConnectOutcome::PendingHostKeyConfirmation { decision, .. } => {
            panic!("expected Mismatch, got {decision:?}")
        }
        ConnectOutcome::Connected(_) => panic!("connected to a server with a changed host key"),
    }
    assert_no_auth(&log_since(scenario, log_start));
    // Nothing was stored by the mismatch.
    assert_eq!(store.stored(port, &algorithm_of(&raw)), Some(raw));
}

async fn fingerprint_matches_ssh_keygen(scenario: &str, pub_file: &str, algo: &str) {
    let port = scenario_port(scenario);
    let store = Arc::new(Store::default());
    let (raw, reported) = expect_unknown(connect(&store, port).await, port);
    assert_eq!(algorithm_of(&raw), algo);
    assert_eq!(reported, server_fingerprint(scenario, pub_file));
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fingerprint_equals_ssh_keygen_ed25519() {
    fingerprint_matches_ssh_keygen(
        "hostkey-ed25519",
        "/etc/ssh/ssh_host_ed25519_key.pub",
        "ssh-ed25519",
    )
    .await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fingerprint_equals_ssh_keygen_ecdsa_p256() {
    fingerprint_matches_ssh_keygen(
        "hostkey-ecdsa",
        "/etc/ssh/ssh_host_ecdsa_key.pub",
        "ecdsa-sha2-nistp256",
    )
    .await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fingerprint_equals_ssh_keygen_rsa() {
    fingerprint_matches_ssh_keygen("hostkey-rsa", "/etc/ssh/ssh_host_rsa_key.pub", "ssh-rsa").await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn key_type_switch_is_unknown_never_silent_trust() {
    let scenario = "hostkey-switch";
    let port = scenario_port(scenario);
    let store = Arc::new(Store::default());

    let (ed, _) = expect_unknown(connect(&store, port).await, port);
    assert_eq!(algorithm_of(&ed), "ssh-ed25519");
    store.trust(HOST, port, &ed).unwrap();
    assert!(matches!(
        connect(&store, port).await,
        ConnectOutcome::Connected(_)
    ));

    let port = restart_and_wait(scenario, &store, port).await; // now only the RSA host key is offered
    let log_start = daemon_log(scenario).len();

    // Spec 0005 §6.1: an algorithm without a stored key counts as unknown.
    let (rsa, reported) = expect_unknown(connect(&store, port).await, port);
    assert_eq!(algorithm_of(&rsa), "ssh-rsa");
    assert_eq!(
        reported,
        server_fingerprint(scenario, "/etc/ssh/ssh_host_rsa_key.pub")
    );
    assert_no_auth(&log_since(scenario, log_start));
    assert_eq!(
        store.trusted.lock().unwrap().len(),
        1,
        "RSA was trusted silently"
    );
    assert_eq!(store.stored(port, "ssh-rsa"), None);
    assert_eq!(store.stored(port, "ssh-ed25519"), Some(ed));
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trust_for_one_port_does_not_trust_the_other_port() {
    let (a, b) = ("hostkey-port-a", "hostkey-port-b");
    let (port_a, port_b) = (scenario_port(a), scenario_port(b));
    assert_ne!(port_a, port_b);
    let store = Arc::new(Store::default());

    let (raw_a, _) = expect_unknown(connect(&store, port_a).await, port_a);
    store.trust(HOST, port_a, &raw_a).unwrap();
    assert!(matches!(
        connect(&store, port_a).await,
        ConnectOutcome::Connected(_)
    ));

    let log_start = daemon_log(b).len();
    let (raw_b, _) = expect_unknown(connect(&store, port_b).await, port_b);
    assert_ne!(raw_a, raw_b, "the two daemons must have different keys");
    assert_no_auth(&log_since(b, log_start));
    assert_eq!(store.stored(port_b, &algorithm_of(&raw_b)), None);
}

/// Pins the current behaviour for a host certificate: the key inside the
/// certificate counts (Spec 0005 §6.2), the normal "unknown" prompt appears
/// with the fingerprint of the server's public key, nothing is trusted silently
/// and no credentials are sent.
#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_certificate_is_checked_as_plain_key_with_normal_prompt() {
    let scenario = "hostkey-cert";
    let port = scenario_port(scenario);
    let store = Arc::new(Store::default());
    let log_start = daemon_log(scenario).len();

    let (_, reported) = expect_unknown(connect(&store, port).await, port);
    assert_eq!(
        reported,
        server_fingerprint(scenario, "/etc/ssh/ssh_host_ed25519_key.pub")
    );
    assert_no_auth(&log_since(scenario, log_start));
    assert!(store.trusted.lock().unwrap().is_empty());
}
