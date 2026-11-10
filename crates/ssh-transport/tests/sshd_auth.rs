//! End-to-end authentication against real OpenSSH (issue #1176): password,
//! private key, identity file, certificate and ssh-agent, each with its
//! failure cases. Scenarios `auth-keys` and `auth-cert`. Run through
//! `tests/sshd/run.sh`. Secrets are never printed in assertion messages.
#![cfg(unix)]
#![expect(clippy::unwrap_used, reason = "integration test")]

mod sshd_support;

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    ConnectLog, ConnectionTarget, Hop, HostKeyStore, KeyFileContent, KeyFileError, KeyFileFacts,
    KeyFileReader, NoPrompter, SshError, SshTransport,
};
use ssh_transport::{classify_openssh_key, KeyClassification};
use sshd_support::{daemon_log, keys_dir, scenario_port, step, Keys, USER};

const KEYS: &str = "auth-keys";
const CERT: &str = "auth-cert";

// ---------------------------------------------------------------- helpers

struct MapCreds(HashMap<String, String>);

impl MapCreds {
    fn new(entries: &[(&str, &str)]) -> Self {
        Self(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }
}

impl CredentialStore for MapCreds {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        match self.0.get(r.as_str()) {
            Some(v) => Ok(SecretString::from(v.clone())),
            None => Err(CredentialError::NotFound(r.clone())),
        }
    }
    fn set(&self, _r: &CredentialRef, _v: SecretString) -> CredentialResult<()> {
        Ok(())
    }
    fn delete(&self, _r: &CredentialRef) -> CredentialResult<()> {
        Ok(())
    }
}

/// Reads identity files straight from disk (no permission enforcement needed:
/// the files are created by the test with mode 0600).
struct FileReader;
impl KeyFileReader for FileReader {
    fn read(&self, path: &str, _enforce: bool) -> Result<KeyFileContent, KeyFileError> {
        let text = std::fs::read_to_string(path).map_err(|_| KeyFileError::NotFound {
            path: path.to_string(),
        })?;
        match classify_openssh_key(text.as_bytes()) {
            KeyClassification::Valid { encrypted } => Ok(KeyFileContent {
                key: SecretString::from(text),
                encrypted,
            }),
            KeyClassification::Invalid => Err(KeyFileError::InvalidKey {
                path: path.to_string(),
            }),
        }
    }
    fn inspect(&self, path: &str) -> KeyFileFacts {
        panic!("unexpected key file inspection of {path}");
    }
}

fn run(cmd: &mut Command) {
    let out = cmd.output().unwrap_or_else(|e| panic!("{cmd:?}: {e}"));
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn keygen(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    run(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", name, "-f"])
        .arg(&path));
    path
}

/// Signs `<key>.pub` with `ca`; returns the path of the certificate.
fn sign(ca: &Path, key: &Path, principals: &str, validity: &str) -> PathBuf {
    let mut pubkey = key.as_os_str().to_owned();
    pubkey.push(".pub");
    run(Command::new("ssh-keygen")
        .args(["-q", "-s"])
        .arg(ca)
        .args(["-I", "test-cert", "-n", principals, "-V", validity])
        .arg(&pubkey));
    let mut cert = key.as_os_str().to_owned();
    cert.push("-cert.pub");
    PathBuf::from(cert)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// A line from the middle of a private key file: key material that must never
/// show up in an error or the connect log.
fn key_fragment(path: &Path) -> String {
    let text = read(path);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len() / 2].to_string()
}

fn hop_with(port: u16, auth: AuthMethod) -> Hop {
    Hop {
        host: "127.0.0.1".to_string(),
        port,
        username: USER.to_string(),
        auth,
    }
}

struct Attempt {
    result: Result<Box<dyn SshTransport>, SshError>,
    /// The connect log (steps and their results), rendered for secret checks.
    log: String,
}

impl Attempt {
    fn err(self) -> (SshError, String) {
        match self.result {
            Ok(_) => panic!("login unexpectedly succeeded"),
            Err(e) => (e, self.log),
        }
    }
    async fn ok(self) {
        let mut t = match self.result {
            Ok(t) => t,
            Err(e) => panic!("login failed: {e:?}"),
        };
        let out = step("execute(echo ok)", t.execute("echo ok"))
            .await
            .unwrap();
        assert_eq!(out.stdout, b"ok\n");
    }
}

async fn attempt(
    port: u16,
    auth: AuthMethod,
    creds: &MapCreds,
    files: &(dyn KeyFileReader + Send + Sync),
) -> Attempt {
    let keys: Arc<dyn HostKeyStore> = Arc::new(Keys::default());
    let target = ConnectionTarget {
        hops: vec![hop_with(port, auth)],
    };
    let log = ConnectLog::new();
    for _ in 0..2 {
        let outcome = step(
            "connect()",
            ssh_transport::connect_with_log(&target, creds, files, keys.clone(), &NoPrompter, &log),
        )
        .await;
        match outcome {
            Err(e) => {
                return Attempt {
                    result: Err(e),
                    log: format!("{:?}", log.progress_snapshot()),
                }
            }
            Ok(ssh_transport::ConnectOutcome::Connected(t)) => {
                return Attempt {
                    result: Ok(t),
                    log: format!("{:?}", log.progress_snapshot()),
                }
            }
            Ok(ssh_transport::ConnectOutcome::PendingHostKeyConfirmation {
                host,
                port,
                raw_key,
                ..
            }) => keys.trust(&host, port, &raw_key).unwrap(),
        }
    }
    panic!("host key confirmation did not settle");
}

fn assert_no_secret(what: &str, haystack: &str, secrets: &[&str]) {
    for s in secrets {
        assert!(!s.is_empty());
        assert!(
            !haystack.contains(s),
            "{what} contains secret material (not shown)"
        );
    }
}

fn is_credential_error(e: &SshError) -> bool {
    matches!(e, SshError::CredentialResolutionFailed(_))
}

/// Waits until `pred` holds for the daemon log written since `offset`.
async fn log_since_until(scenario: &str, offset: usize, pred: impl Fn(&str) -> bool) -> String {
    for _ in 0..50 {
        let log = daemon_log(scenario);
        let new = log.get(offset..).unwrap_or("").to_string();
        if pred(&new) {
            return new;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("daemon log of {scenario} did not show the expected entry");
}

// ---------------------------------------------------- password / key / file

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn password_right_logs_in() {
    let creds = MapCreds::new(&[("pw", sshd_support::PASS)]);
    let auth = AuthMethod::Password {
        credential_ref: CredentialRef::new("pw"),
    };
    attempt(scenario_port(KEYS), auth, &creds, &FileReader)
        .await
        .ok()
        .await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_password_fails_without_leaking() {
    let wrong = "definitely-wrong-pw-1176";
    let creds = MapCreds::new(&[("pw", wrong)]);
    let auth = AuthMethod::Password {
        credential_ref: CredentialRef::new("pw"),
    };
    let (e, log) = attempt(scenario_port(KEYS), auth, &creds, &FileReader)
        .await
        .err();
    assert!(matches!(e, SshError::AuthenticationFailed), "got {e:?}");
    assert_no_secret("error", &format!("{e} {e:?}"), &[wrong]);
    assert_no_secret("connect log", &log, &[wrong]);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authorized_key_logs_in_and_rejected_key_fails() {
    let dir = tempfile::tempdir().unwrap();
    let port = scenario_port(KEYS);
    let good = read(&keys_dir().join("user_ed25519"));
    let auth = || AuthMethod::PrivateKey {
        credential_ref: CredentialRef::new("key"),
        passphrase_ref: None,
    };
    attempt(port, auth(), &MapCreds::new(&[("key", &good)]), &FileReader)
        .await
        .ok()
        .await;
    // RSA key (also authorized): covers the rsa-sha2 signature path.
    let rsa = read(&keys_dir().join("user_rsa"));
    attempt(port, auth(), &MapCreds::new(&[("key", &rsa)]), &FileReader)
        .await
        .ok()
        .await;

    let rejected_path = keygen(dir.path(), "rejected");
    let rejected = read(&rejected_path);
    let (e, log) = attempt(
        port,
        auth(),
        &MapCreds::new(&[("key", &rejected)]),
        &FileReader,
    )
    .await
    .err();
    assert!(matches!(e, SshError::AuthenticationFailed), "got {e:?}");
    let fragment = key_fragment(&rejected_path);
    assert_no_secret("error", &format!("{e} {e:?}"), &[&fragment]);
    assert_no_secret("connect log", &log, &[&fragment]);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn encrypted_identity_file_right_and_wrong_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    // Same (authorized) key, now protected by a passphrase.
    let path = dir.path().join("enc_key");
    std::fs::copy(keys_dir().join("user_ed25519"), &path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let right = "right-passphrase-1176";
    let wrong = "wrong-passphrase-1176";
    run(Command::new("ssh-keygen")
        .args(["-q", "-p", "-P", "", "-N", right, "-f"])
        .arg(&path));

    let port = scenario_port(KEYS);
    let auth = || AuthMethod::IdentityFile {
        path: path.to_str().unwrap().to_string(),
        passphrase_ref: Some(CredentialRef::new("pass")),
    };
    attempt(
        port,
        auth(),
        &MapCreds::new(&[("pass", right)]),
        &FileReader,
    )
    .await
    .ok()
    .await;

    let (e, log) = attempt(
        port,
        auth(),
        &MapCreds::new(&[("pass", wrong)]),
        &FileReader,
    )
    .await
    .err();
    // Distinguishable from a server-side rejection.
    assert!(is_credential_error(&e), "got {e:?}");
    let fragment = key_fragment(&path);
    assert_no_secret("error", &format!("{e} {e:?}"), &[right, wrong, &fragment]);
    assert_no_secret("connect log", &log, &[right, wrong, &fragment]);
}

// ------------------------------------------------------------ certificates

async fn cert_attempt(cert: &Path, key: &Path) -> Attempt {
    let creds = MapCreds::new(&[("cert", &read(cert)), ("key", &read(key))]);
    let auth = AuthMethod::Certificate {
        cert_ref: CredentialRef::new("cert"),
        key_ref: CredentialRef::new("key"),
    };
    attempt(scenario_port(CERT), auth, &creds, &FileReader).await
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn valid_user_certificate_logs_in() {
    let keys = keys_dir();
    cert_attempt(
        &keys.join("user_ed25519-cert.pub"),
        &keys.join("user_ed25519"),
    )
    .await
    .ok()
    .await;
}

/// Failing certificate: `AuthenticationFailed` and no password attempt in the
/// daemon log (no silent fallback).
async fn assert_cert_rejected(cert: &Path, key: &Path) {
    let offset = daemon_log(CERT).len();
    let (e, log) = cert_attempt(cert, key).await.err();
    assert!(matches!(e, SshError::AuthenticationFailed), "got {e:?}");
    let fragment = key_fragment(key);
    assert_no_secret("error", &format!("{e} {e:?}"), &[&fragment]);
    assert_no_secret("connect log", &log, &[&fragment]);
    let new = log_since_until(CERT, offset, |l| {
        l.contains("Failed publickey") || l.contains("Certificate invalid")
    })
    .await;
    assert!(
        !new.contains("password for") && !new.contains("Accepted"),
        "daemon log shows a password attempt or a login:\n{new}"
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_certificate_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let key = keygen(dir.path(), "expired");
    let cert = sign(&keys_dir().join("ca"), &key, USER, "20200101:20200102");
    assert_cert_rejected(&cert, &key).await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn certificate_from_another_ca_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let other_ca = keygen(dir.path(), "other_ca");
    let key = keygen(dir.path(), "foreign");
    let cert = sign(&other_ca, &key, USER, "-5m:+1d");
    assert_cert_rejected(&cert, &key).await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn certificate_with_wrong_principal_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let key = keygen(dir.path(), "principal");
    let cert = sign(&keys_dir().join("ca"), &key, "someone-else", "-5m:+1d");
    assert_cert_rejected(&cert, &key).await;
}

// ------------------------------------------------------------------- agent

/// A private `ssh-agent` on a temporary socket. `SSH_AUTH_SOCK` is set for the
/// lifetime of the guard only.
struct Agent {
    child: Child,
    dir: tempfile::TempDir,
    sock: PathBuf,
}

impl Agent {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("agent.sock");
        let child = Command::new("ssh-agent")
            .args(["-D", "-a"])
            .arg(&sock)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        for _ in 0..50 {
            if sock.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(sock.exists(), "ssh-agent did not create its socket");
        std::env::set_var("SSH_AUTH_SOCK", &sock);
        Self { child, dir, sock }
    }

    fn add(&self, key: &Path) {
        run(Command::new("ssh-add")
            .env("SSH_AUTH_SOCK", &self.sock)
            .arg(key)
            .stdin(Stdio::null()));
    }

    /// `ssh-add -x`: locks the agent; the password comes from an askpass
    /// script.
    fn lock(&self) {
        let script = self.dir.path().join("askpass.sh");
        std::fs::write(&script, "#!/bin/sh\necho lock-password-1176\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        run(Command::new("ssh-add")
            .env("SSH_AUTH_SOCK", &self.sock)
            .env("SSH_ASKPASS", &script)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .arg("-x")
            .stdin(Stdio::null()));
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        std::env::remove_var("SSH_AUTH_SOCK");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn agent_attempt() -> Attempt {
    attempt(
        scenario_port(KEYS),
        AuthMethod::Agent,
        &MapCreds::new(&[]),
        &FileReader,
    )
    .await
}

fn assert_agent_error(e: &SshError) {
    match e {
        SshError::CredentialResolutionFailed(msg) => {
            assert!(msg.contains("SSH-Agent"), "unclear message: {msg}");
        }
        other => panic!("expected a clear agent error, got {other:?}"),
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_with_right_key_logs_in() {
    let agent = Agent::start().await;
    agent.add(&keys_dir().join("user_ed25519"));
    agent_attempt().await.ok().await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_agent_gives_clear_error() {
    let _agent = Agent::start().await;
    let (e, _) = agent_attempt().await.err();
    assert_agent_error(&e);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn locked_agent_gives_clear_error() {
    let agent = Agent::start().await;
    agent.add(&keys_dir().join("user_ed25519"));
    agent.lock();
    let (e, _) = agent_attempt().await.err();
    assert_agent_error(&e);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_agent_socket_gives_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("SSH_AUTH_SOCK", dir.path().join("no-such.sock"));
    let result = agent_attempt().await;
    std::env::remove_var("SSH_AUTH_SOCK");
    let (e, _) = result.err();
    assert_agent_error(&e);
}

/// Issue #1217: the right key as the agent's second identity is found.
#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_with_right_key_second_logs_in() {
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start().await;
    agent.add(&keygen(dir.path(), "wrong0"));
    agent.add(&keys_dir().join("user_ed25519"));
    agent_attempt().await.ok().await;
}

/// Issue #1229: the CA-only server accepts the agent's certificate identity
/// (the plain key is rejected first); a wrong key before it does not matter.
#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_certificate_identity_logs_in_on_ca_only_server() {
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start().await;
    agent.add(&keygen(dir.path(), "wrong_before_cert"));
    let key = keygen(dir.path(), "agentcert");
    sign(&keys_dir().join("ca"), &key, USER, "-5m:+1d");
    agent.add(&key); // ssh-add also loads `<key>-cert.pub`
    attempt(
        scenario_port(CERT),
        AuthMethod::Agent,
        &MapCreds::new(&[]),
        &FileReader,
    )
    .await
    .ok()
    .await;
}

/// Issue #1229: a certificate from another CA ends in `AuthenticationFailed`.
#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_certificate_from_another_ca_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start().await;
    let other_ca = keygen(dir.path(), "other_ca_agent");
    let key = keygen(dir.path(), "agentforeign");
    sign(&other_ca, &key, USER, "-5m:+1d");
    agent.add(&key);
    let (e, log) = attempt(
        scenario_port(CERT),
        AuthMethod::Agent,
        &MapCreds::new(&[]),
        &FileReader,
    )
    .await
    .err();
    assert!(matches!(e, SshError::AuthenticationFailed), "got {e:?}");
    let fragment = key_fragment(&key);
    assert_no_secret("error", &format!("{e} {e:?}"), &[&fragment]);
    assert_no_secret("connect log", &log, &[&fragment]);
}

/// Only wrong keys (fewer than `MaxAuthTries`): `AuthenticationFailed`.
#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_with_only_wrong_keys_fails_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start().await;
    for i in 0..3 {
        agent.add(&keygen(dir.path(), &format!("wrong{i}")));
    }
    let (e, _) = agent_attempt().await.err();
    assert!(matches!(e, SshError::AuthenticationFailed), "got {e:?}");
}

/// With seven wrong keys before the right one, every identity is tried in
/// order until the server's `MaxAuthTries 6` ends the login: an
/// authentication-side error (not a credential error, not a hang), and the
/// right key is never reached.
#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_with_seven_wrong_keys_before_the_right_one_hits_max_auth_tries() {
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start().await;
    for i in 0..7 {
        agent.add(&keygen(dir.path(), &format!("wrong{i}")));
    }
    agent.add(&keys_dir().join("user_ed25519"));
    let offset = daemon_log(KEYS).len();
    let (e, _) = tokio::time::timeout(std::time::Duration::from_secs(60), agent_attempt())
        .await
        .expect("login must not hang")
        .err();
    assert!(!is_credential_error(&e), "got {e:?}");
    let new = log_since_until(KEYS, offset, |l| {
        l.contains("Too many authentication failures")
    })
    .await;
    assert!(
        new.contains("Too many authentication failures"),
        "MaxAuthTries not reached:\n{new}"
    );
}

/// Issue #1217: with the right key first, exactly one publickey attempt is
/// made and accepted; no wrong identity is offered before it.
#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_with_right_key_first_makes_a_single_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent::start().await;
    agent.add(&keys_dir().join("user_ed25519"));
    agent.add(&keygen(dir.path(), "wrong0"));
    let offset = daemon_log(KEYS).len();
    agent_attempt().await.ok().await;
    let new = log_since_until(KEYS, offset, |l| l.contains("Accepted publickey")).await;
    assert_eq!(new.matches("Accepted publickey").count(), 1, "{new}");
    assert!(!new.contains("Failed publickey"), "{new}");
}
