//! Issue #1175 (Spec 0005, "Outdated algorithms"): the policy from #1173
//! against real daemons: OpenSSH 7.4p1 (`openssh-legacy`) and a current
//! OpenSSH, with restricted algorithm lists. Scenarios are `algo-*`.
//! Run through `tests/sshd/run.sh`.
//!
//! The legacy daemon does not support strict key exchange, so every
//! connection to it also carries the Terrapin entry (`STRICT_KEX_MISSING`) in
//! its single per-hop warning; the tests therefore look at the other names.
#![expect(clippy::unwrap_used, reason = "integration test")]

mod sshd_support;

use std::sync::Arc;

use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    AlgorithmKind, ConnectLog, ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, NoPrompter,
    SshError, STRICT_KEX_MISSING,
};
use ssh_transport::ConnectOutcome;
use sshd_support::{
    daemon_log, keys_dir, scenario_port, step, Keys, NoKeyFiles, PASS, PASS_REF, USER,
};

const KEY_REF: &str = "sshd-test-rsa-key";

/// Password and the per-run RSA user key.
struct AllCreds;
impl CredentialStore for AllCreds {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        match r.as_str() {
            PASS_REF => Ok(SecretString::from(PASS.to_string())),
            KEY_REF => Ok(SecretString::from(
                std::fs::read_to_string(keys_dir().join("user_rsa")).unwrap(),
            )),
            _ => Err(CredentialError::NotFound(r.clone())),
        }
    }
    fn set(&self, _r: &CredentialRef, _v: SecretString) -> CredentialResult<()> {
        Ok(())
    }
    fn delete(&self, _r: &CredentialRef) -> CredentialResult<()> {
        Ok(())
    }
}

struct Attempt {
    result: Result<ConnectOutcome, SshError>,
    log: ConnectLog,
}

impl Attempt {
    /// Algorithm names of the (single) warning of the hop; empty if none.
    fn warned(&self) -> Vec<String> {
        let w = self.log.warnings();
        assert!(w.len() <= 1, "one warning per hop expected: {w:?}");
        w.into_iter()
            .next()
            .map(|w| w.algorithms)
            .unwrap_or_default()
    }
    /// Warned names without the Terrapin entry.
    fn warned_algorithms(&self) -> Vec<String> {
        let mut v = self.warned();
        v.retain(|a| a != STRICT_KEX_MISSING);
        v
    }
    fn assert_connected(&self) {
        assert!(
            matches!(self.result, Ok(ConnectOutcome::Connected(_))),
            "expected a connection, got {:?}",
            self.result.as_ref().err()
        );
    }
}

fn hop(scenario: &str, auth: AuthMethod) -> Hop {
    Hop {
        host: "127.0.0.1".into(),
        port: scenario_port(scenario),
        username: USER.into(),
        auth,
    }
}

fn password() -> AuthMethod {
    AuthMethod::Password {
        credential_ref: CredentialRef::new(PASS_REF),
    }
}

fn rsa_key() -> AuthMethod {
    AuthMethod::PrivateKey {
        credential_ref: CredentialRef::new(KEY_REF),
        passphrase_ref: None,
    }
}

/// Connects once; an unknown host key is trusted and the connect repeated.
/// The returned log belongs to the last attempt.
async fn connect(scenario: &str, auth: AuthMethod) -> Attempt {
    let keys: Arc<dyn HostKeyStore> = Arc::new(Keys::default());
    let target = ConnectionTarget {
        hops: vec![hop(scenario, auth)],
    };
    for _ in 0..2 {
        let log = ConnectLog::new();
        let result = step(
            "connect_with_log()",
            ssh_transport::connect_with_log(
                &target,
                &AllCreds,
                &NoKeyFiles,
                keys.clone(),
                &NoPrompter,
                &log,
            ),
        )
        .await;
        match result {
            Ok(ConnectOutcome::PendingHostKeyConfirmation {
                host,
                port,
                raw_key,
                decision,
            }) => {
                assert!(matches!(decision, HostKeyDecision::Unknown { .. }));
                keys.trust(&host, port, &raw_key).unwrap();
            }
            result => return Attempt { result, log },
        }
    }
    panic!("host key confirmation was asked twice");
}

/// #1173: an algorithm that russh implements connects with a warning naming
/// it; one that russh lacks fails with `NoCommonAlgorithm` of that kind.
fn assert_warning_or_no_common(attempt: &Attempt, names: &[&str], kind: AlgorithmKind) {
    match &attempt.result {
        Ok(ConnectOutcome::Connected(_)) => {
            let warned = attempt.warned_algorithms();
            assert!(
                warned.len() == 1 && names.contains(&warned[0].as_str()),
                "expected a warning naming one of {names:?}, got {warned:?}"
            );
        }
        Err(SshError::NoCommonAlgorithm { kind: k, .. }) => assert_eq!(*k, kind),
        other => panic!("unexpected result: {:?}", other.as_ref().err()),
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn current_server_with_only_rsa_sha2_signatures_accepts_rsa_key_without_warning() {
    let a = connect("algo-current-rsa-sha2", rsa_key()).await;
    a.assert_connected();
    assert!(a.log.warnings().is_empty(), "{:?}", a.log.warnings());
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openssh_7_4_accepts_rsa_key_and_warns_about_ssh_rsa_exactly_when_used() {
    let a = connect("algo-legacy-default", rsa_key()).await;
    a.assert_connected();
    // The daemon (LogLevel DEBUG3) names the signature algorithm it was
    // offered; the warning must name `ssh-rsa` iff that was the one used.
    let log = daemon_log("algo-legacy-default");
    assert!(log.contains("Accepted publickey"), "{log}");
    let used_sha1 = log.contains("authenticated 1 pkalg ssh-rsa");
    let used_sha2 = log.contains("authenticated 1 pkalg rsa-sha2-256")
        || log.contains("authenticated 1 pkalg rsa-sha2-512");
    assert!(
        used_sha1 != used_sha2,
        "could not tell the signature algorithm from the daemon log: {log}"
    );
    let expected: &[&str] = if used_sha1 { &["ssh-rsa"] } else { &[] };
    assert_eq!(a.warned_algorithms(), expected);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssh_rsa_only_host_key_and_user_key_connects_with_ssh_rsa_warning() {
    let a = connect("algo-legacy-ssh-rsa-only", rsa_key()).await;
    a.assert_connected();
    assert_eq!(a.warned_algorithms(), ["ssh-rsa"]);
    assert!(daemon_log("algo-legacy-ssh-rsa-only").contains("Accepted publickey"));
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kex_group1_sha1_only() {
    let a = connect("algo-legacy-kex-group1", password()).await;
    assert_warning_or_no_common(
        &a,
        &["diffie-hellman-group1-sha1"],
        AlgorithmKind::KeyExchange,
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kex_group14_sha1_only() {
    let a = connect("algo-legacy-kex-group14", password()).await;
    assert_warning_or_no_common(
        &a,
        &["diffie-hellman-group14-sha1"],
        AlgorithmKind::KeyExchange,
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cbc_ciphers_only() {
    let a = connect("algo-legacy-cbc", password()).await;
    assert_warning_or_no_common(&a, &["aes128-cbc", "3des-cbc"], AlgorithmKind::Cipher);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hmac_sha1_only() {
    let a = connect("algo-legacy-hmac-sha1", password()).await;
    assert_warning_or_no_common(&a, &["hmac-sha1"], AlgorithmKind::Mac);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssh_dss_only_host_key_is_no_common_algorithm_before_any_authentication() {
    let a = connect("algo-legacy-dss-only", password()).await;
    match &a.result {
        Err(SshError::NoCommonAlgorithm { kind, .. }) => assert_eq!(*kind, AlgorithmKind::HostKey),
        other => panic!("expected NoCommonAlgorithm, got {:?}", other.as_ref().err()),
    }
    let log = daemon_log("algo-legacy-dss-only");
    for attempt in [
        "Failed password",
        "Accepted ",
        "Failed none",
        "authenticating user",
    ] {
        assert!(
            !log.contains(attempt),
            "authentication attempt seen ({attempt}): {log}"
        );
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openssh_7_4_without_strict_kex_gets_terrapin_warning() {
    let a = connect("algo-legacy-default", password()).await;
    a.assert_connected();
    assert!(
        a.warned().contains(&STRICT_KEX_MISSING.to_string()),
        "{:?}",
        a.warned()
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn current_openssh_has_no_terrapin_warning() {
    let a = connect("algo-current-default", password()).await;
    a.assert_connected();
    assert!(a.log.warnings().is_empty(), "{:?}", a.log.warnings());
}
