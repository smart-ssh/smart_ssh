//! Shared helpers for the scenario-based end-to-end tests against real sshd
//! containers (`tests/sshd/README.md`). Keep it small: helpers that only one
//! area needs belong into that area's `sshd_*.rs` file.
//!
//! The runner `tests/sshd/run.sh` exports, per scenario `<name>` (upper case,
//! `-` as `_`): `SSHD_<NAME>_PORT` (only if the scenario publishes its port)
//! and `SSHD_<NAME>_CONTAINER`, plus `SSHD_KEYS_DIR`.
#![expect(dead_code, reason = "each sshd_*.rs target uses a different subset")]
#![expect(clippy::unwrap_used, reason = "integration test")]

use std::collections::HashMap;
use std::future::Future;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::NoPrompter;
use ssh_manager_core::ssh::{
    ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, KeyFileContent, KeyFileError,
    KeyFileFacts, KeyFileReader, SshError, SshTransport,
};
use ssh_transport::ConnectOutcome;

/// Throw-away account baked into every test image.
pub const USER: &str = "testuser";
pub const PASS: &str = "testpass";
/// Credential reference that `Creds` resolves to `PASS`.
pub const PASS_REF: &str = "sshd-test-password";
pub const STEP: Duration = Duration::from_secs(30);

fn env_name(scenario: &str) -> String {
    scenario.to_uppercase().replace('-', "_")
}

fn required_env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| {
        panic!(
            "{key} is not set: run the tests through tests/sshd/run.sh (see tests/sshd/README.md)"
        )
    })
}

/// Host port of a scenario (127.0.0.1) that is published to the host.
pub fn scenario_port(scenario: &str) -> u16 {
    required_env(&format!("SSHD_{}_PORT", env_name(scenario)))
        .parse()
        .unwrap()
}

pub fn scenario_container(scenario: &str) -> String {
    required_env(&format!("SSHD_{}_CONTAINER", env_name(scenario)))
}

/// Directory with the per-run keys, CA and certificates.
pub fn keys_dir() -> std::path::PathBuf {
    required_env("SSHD_KEYS_DIR").into()
}

/// Every step has a timeout: better to fail than to hang.
pub async fn step<T>(what: &str, fut: impl Future<Output = T>) -> T {
    tokio::time::timeout(STEP, fut)
        .await
        .unwrap_or_else(|_| panic!("timeout after {STEP:?}: {what}"))
}

pub struct NoKeyFiles;
impl KeyFileReader for NoKeyFiles {
    fn read(&self, path: &str, _e: bool) -> Result<KeyFileContent, KeyFileError> {
        panic!("unexpected key file read of {path}");
    }
    fn inspect(&self, path: &str) -> KeyFileFacts {
        panic!("unexpected key file inspection of {path}");
    }
}

pub struct Creds;
impl CredentialStore for Creds {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        if r.as_str() == PASS_REF {
            Ok(SecretString::from(PASS.to_string()))
        } else {
            Err(CredentialError::NotFound(r.clone()))
        }
    }
    fn set(&self, _r: &CredentialRef, _v: SecretString) -> CredentialResult<()> {
        Ok(())
    }
    fn delete(&self, _r: &CredentialRef) -> CredentialResult<()> {
        Ok(())
    }
}

/// In-memory host key store, initially empty: every host key is unknown until
/// it is explicitly confirmed with `trust()`.
#[derive(Default)]
pub struct Keys {
    known: Mutex<HashMap<(String, u16), Vec<u8>>>,
}
impl HostKeyStore for Keys {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        match self.known.lock().unwrap().get(&(host.to_string(), port)) {
            None => HostKeyDecision::Unknown {
                fingerprint: format!("{key:02x?}"),
            },
            Some(s) if s.as_slice() == key => HostKeyDecision::Trusted,
            Some(s) => HostKeyDecision::Mismatch {
                expected_fingerprint: format!("{s:02x?}"),
                actual_fingerprint: format!("{key:02x?}"),
            },
        }
    }
    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError> {
        self.known
            .lock()
            .unwrap()
            .insert((host.to_string(), port), key.to_vec());
        Ok(())
    }
}

/// Password hop for `host:port`.
pub fn hop(host: &str, port: u16) -> Hop {
    Hop {
        host: host.to_string(),
        port,
        username: USER.to_string(),
        auth: AuthMethod::Password {
            credential_ref: CredentialRef::new(PASS_REF),
        },
    }
}

/// Connects over `hops`. Every `PendingHostKeyConfirmation` concerns exactly
/// one hop; it is confirmed (`trust()` for exactly that key) and the connect
/// is retried. Returns the transport and the confirmed `(host, port)` list.
pub async fn connect_confirming(hops: Vec<Hop>) -> (Box<dyn SshTransport>, Vec<(String, u16)>) {
    let expected = hops.len();
    let keys: Arc<dyn HostKeyStore> = Arc::new(Keys::default());
    let target = ConnectionTarget { hops };
    let mut confirmed: Vec<(String, u16)> = Vec::new();
    for _ in 0..=expected {
        match step(
            "connect()",
            ssh_transport::connect(&target, &Creds, &NoKeyFiles, keys.clone(), &NoPrompter),
        )
        .await
        .expect("connect() should succeed or pause")
        {
            ConnectOutcome::Connected(t) => return (t, confirmed),
            ConnectOutcome::PendingHostKeyConfirmation {
                host,
                port,
                raw_key,
                decision,
            } => {
                assert!(
                    matches!(decision, HostKeyDecision::Unknown { .. }),
                    "expected Unknown for {host}:{port}, got {decision:?}"
                );
                assert!(
                    !confirmed.contains(&(host.clone(), port)),
                    "hop {host}:{port} was asked twice"
                );
                keys.trust(&host, port, &raw_key).unwrap();
                confirmed.push((host, port));
            }
        }
    }
    panic!("connection not established after {expected} confirmations");
}

/// Direct password connection to a published scenario.
pub async fn connect_scenario(scenario: &str) -> Box<dyn SshTransport> {
    connect_confirming(vec![hop("127.0.0.1", scenario_port(scenario))])
        .await
        .0
}

fn docker(args: &[&str]) -> std::process::Output {
    let out = Command::new("docker")
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("docker {args:?}: {e}"));
    assert!(
        out.status.success(),
        "docker {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Daemon log (stdout and stderr) of a scenario container.
pub fn daemon_log(scenario: &str) -> String {
    let out = docker(&["logs", &scenario_container(scenario)]);
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Daemon log once it is stable: unchanged for `STABLE_FOR`, so late-flushed
/// lines are included. Bounded: after `STABLE_MAX` the current log is
/// returned anyway (no endless loop).
pub fn stable_daemon_log(scenario: &str) -> String {
    const STABLE_FOR: std::time::Duration = std::time::Duration::from_millis(500);
    const STABLE_MAX: std::time::Duration = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();
    let mut last = daemon_log(scenario);
    let mut since = std::time::Instant::now();
    while since.elapsed() < STABLE_FOR && start.elapsed() < STABLE_MAX {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let now = daemon_log(scenario);
        if now != last {
            last = now;
            since = std::time::Instant::now();
        }
    }
    last
}

/// Stable log lines written after `baseline` (a former `stable_daemon_log`).
pub fn stable_log_since(scenario: &str, baseline: &str) -> String {
    let log = stable_daemon_log(scenario);
    match log.strip_prefix(baseline) {
        Some(tail) => tail.to_string(),
        None => log,
    }
}

pub fn pause(scenario: &str) {
    docker(&["pause", &scenario_container(scenario)]);
}

pub fn unpause(scenario: &str) {
    docker(&["unpause", &scenario_container(scenario)]);
}

/// Restarts the container (the published host port stays the same).
pub fn restart(scenario: &str) {
    docker(&["restart", "-t", "2", &scenario_container(scenario)]);
}
