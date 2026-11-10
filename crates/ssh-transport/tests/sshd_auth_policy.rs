//! Issue #1177: server-side login policies against real OpenSSH
//! (`authpolicy-*` scenarios): keyboard-interactive, multi-step login,
//! `MaxAuthTries`, login banners, `nologin` shells and locked accounts.
//! Run through `tests/sshd/run.sh`.
#![expect(clippy::unwrap_used, reason = "integration test")]

mod sshd_support;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    AuthPromptRequest, AuthPrompter, ConnectLog, ConnectionTarget, Hop, HostKeyStore,
    KeyFileContent, KeyFileError, KeyFileFacts, KeyFileReader, NoPrompter, PromptCancelled,
    SshError, SshTransport,
};
use ssh_transport::ConnectOutcome;
use sshd_support::{keys_dir, scenario_port, step, Keys, PASS, PASS_REF, USER};

const KEY_PATH: &str = "/test-key";
const WRONG_REF: &str = "wrong-password";

struct Creds;
impl CredentialStore for Creds {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        match r.as_str() {
            PASS_REF => Ok(SecretString::from(PASS.to_string())),
            WRONG_REF => Ok(SecretString::from("definitely-wrong".to_string())),
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

/// Serves the per-run test key (`SSHD_KEYS_DIR/user_ed25519`) for `KEY_PATH`.
struct RunKey;
impl KeyFileReader for RunKey {
    fn read(&self, path: &str, _enforce: bool) -> Result<KeyFileContent, KeyFileError> {
        assert_eq!(path, KEY_PATH);
        let key = std::fs::read_to_string(keys_dir().join("user_ed25519")).unwrap();
        Ok(KeyFileContent {
            key: SecretString::from(key),
            encrypted: false,
        })
    }
    fn inspect(&self, path: &str) -> KeyFileFacts {
        panic!("unexpected key file inspection of {path}");
    }
}

/// Answers from a script, one entry per request; records the number of
/// requests. An empty script cancels.
struct Scripted {
    script: Mutex<VecDeque<Vec<String>>>,
    asked: Mutex<usize>,
}
impl Scripted {
    fn new(script: &[&[&str]]) -> Self {
        Self {
            script: Mutex::new(
                script
                    .iter()
                    .map(|s| s.iter().map(|a| a.to_string()).collect())
                    .collect(),
            ),
            asked: Mutex::new(0),
        }
    }
    fn asked(&self) -> usize {
        *self.asked.lock().unwrap()
    }
}
#[async_trait]
impl AuthPrompter for Scripted {
    async fn prompt(
        &self,
        _request: AuthPromptRequest,
    ) -> Result<Vec<SecretString>, PromptCancelled> {
        *self.asked.lock().unwrap() += 1;
        let next = self.script.lock().unwrap().pop_front();
        next.map(|a| a.into_iter().map(SecretString::from).collect())
            .ok_or(PromptCancelled)
    }
}

fn password(reference: &str) -> AuthMethod {
    AuthMethod::Password {
        credential_ref: CredentialRef::new(reference),
    }
}

fn key() -> AuthMethod {
    AuthMethod::IdentityFile {
        path: KEY_PATH.to_string(),
        passphrase_ref: None,
    }
}

/// One-hop login to a scenario. The host key is unknown at first: it is
/// trusted once (`trust()`), then the connect is retried.
async fn login(
    scenario: &str,
    username: &str,
    auth: AuthMethod,
    prompter: &dyn AuthPrompter,
    log: &ConnectLog,
) -> Result<Box<dyn SshTransport>, SshError> {
    let target = ConnectionTarget {
        hops: vec![Hop {
            host: "127.0.0.1".to_string(),
            port: scenario_port(scenario),
            username: username.to_string(),
            auth,
        }],
    };
    let keys: Arc<dyn HostKeyStore> = Arc::new(Keys::default());
    for _ in 0..2 {
        match step(
            "connect()",
            ssh_transport::connect_with_log(&target, &Creds, &RunKey, keys.clone(), prompter, log),
        )
        .await?
        {
            ConnectOutcome::Connected(t) => return Ok(t),
            ConnectOutcome::PendingHostKeyConfirmation {
                host,
                port,
                raw_key,
                ..
            } => {
                keys.trust(&host, port, &raw_key).unwrap();
            }
        }
    }
    panic!("connection not established after the host key was trusted");
}

async fn echo_ok(mut t: Box<dyn SshTransport>) {
    let out = step("execute(echo ok)", t.execute("echo ok"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"ok\n");
    assert_eq!(out.exit_code, Some(0));
}

fn expect_methods(result: Result<Box<dyn SshTransport>, SshError>, wanted: &str) {
    match result {
        Err(SshError::AuthenticationMethodRequired { methods }) => assert!(
            methods.iter().any(|m| m == wanted),
            "methods {methods:?} should name {wanted}"
        ),
        Err(e) => panic!("expected AuthenticationMethodRequired, got {e:?}"),
        Ok(_) => panic!("login must not succeed"),
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keyboard_interactive_only_rejects_key_profile_naming_method() {
    let prompter = Scripted::new(&[&[PASS]]);
    let t = login(
        "authpolicy-kbdint",
        USER,
        key(),
        &prompter,
        &ConnectLog::new(),
    )
    .await;
    // The server offers only keyboard-interactive, so a key profile is
    // rejected with the method named (no silent switch of method).
    expect_methods(t, "keyboard-interactive");
    assert_eq!(prompter.asked(), 0);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keyboard_interactive_only_with_password_profile_skips_prompter() {
    let prompter = Scripted::new(&[]);
    let t = login(
        "authpolicy-kbdint",
        USER,
        password(PASS_REF),
        &prompter,
        &ConnectLog::new(),
    )
    .await
    .unwrap();
    echo_ok(t).await;
    assert_eq!(
        prompter.asked(),
        0,
        "the stored password answers the prompt"
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keyboard_interactive_only_second_prompt_goes_to_scripted_prompter() {
    // Two prompt rounds: the stored password answers the first, the
    // visible-input prompt of the second can only come from the prompter.
    let prompter = Scripted::new(&[&["tester@example.com"]]);
    let t = login(
        "authpolicy-kbdint-twostep",
        USER,
        password(PASS_REF),
        &prompter,
        &ConnectLog::new(),
    )
    .await
    .unwrap();
    echo_ok(t).await;
    assert_eq!(prompter.asked(), 1, "only the visible-input round is asked");
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publickey_then_password_with_scripted_prompter() {
    let prompter = Scripted::new(&[&[PASS]]);
    let t = login(
        "authpolicy-key-password",
        USER,
        key(),
        &prompter,
        &ConnectLog::new(),
    )
    .await
    .unwrap();
    echo_ok(t).await;
    assert_eq!(prompter.asked(), 1);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publickey_then_keyboard_interactive_with_scripted_prompter() {
    let prompter = Scripted::new(&[&[PASS]]);
    let t = login(
        "authpolicy-key-kbdint",
        USER,
        key(),
        &prompter,
        &ConnectLog::new(),
    )
    .await
    .unwrap();
    echo_ok(t).await;
    assert_eq!(prompter.asked(), 1);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publickey_then_password_without_prompter_names_method() {
    let t = login(
        "authpolicy-key-password",
        USER,
        key(),
        &NoPrompter,
        &ConnectLog::new(),
    )
    .await;
    expect_methods(t, "password");
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publickey_then_keyboard_interactive_without_prompter_names_method() {
    let t = login(
        "authpolicy-key-kbdint",
        USER,
        key(),
        &NoPrompter,
        &ConnectLog::new(),
    )
    .await;
    expect_methods(t, "keyboard-interactive");
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_auth_tries_one_with_wrong_password_fails_without_hang() {
    let t = login(
        "authpolicy-maxtries",
        USER,
        password(WRONG_REF),
        &NoPrompter,
        &ConnectLog::new(),
    )
    .await;
    assert!(
        matches!(t, Err(SshError::AuthenticationFailed)),
        "expected AuthenticationFailed, got {:?}",
        t.err()
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn banner_with_valid_text_does_not_fail_the_connection() {
    let t = login(
        "authpolicy-banner-valid",
        USER,
        password(PASS_REF),
        &NoPrompter,
        &ConnectLog::new(),
    )
    .await
    .unwrap();
    echo_ok(t).await;
}

/// Known-bug tests are `#[ignore = "bug: ..."]`, but the e2e runner passes
/// `--ignored` and would run them. They only execute when
/// `SSHD_E2E_KNOWN_BUGS=1` is set, so the runner stays green until fixed.
fn known_bug_enabled(name: &str) -> bool {
    if std::env::var_os("SSHD_E2E_KNOWN_BUGS").is_some() {
        return true;
    }
    eprintln!("skipping {name}: known bug, set SSHD_E2E_KNOWN_BUGS=1 to run it");
    false
}

/// Upstream russh decodes the banner strictly and aborted the login on invalid
/// UTF-8; the vendored russh (`vendor/russh`, issue #1228) decodes it lossily.
#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn banner_with_invalid_utf8_does_not_fail_the_connection() {
    let t = login(
        "authpolicy-banner-invalid",
        USER,
        password(PASS_REF),
        &NoPrompter,
        &ConnectLog::new(),
    )
    .await
    .unwrap();
    echo_ok(t).await;
}

/// The connect log has no banner step yet, so this cannot pass today.
#[ignore = "bug: login banner text is not recorded in the connect log (no ConnectStep for it)"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn banner_appears_in_connect_log() {
    if !known_bug_enabled("banner_appears_in_connect_log") {
        return;
    }
    for (scenario, needle) in [
        ("authpolicy-banner-valid", "valid banner text"),
        ("authpolicy-banner-invalid", "invalid banner bytes"),
    ] {
        let log = ConnectLog::new();
        let t = login(scenario, USER, password(PASS_REF), &NoPrompter, &log)
            .await
            .unwrap();
        echo_ok(t).await;
        let dump = format!("{:?}", log.snapshot());
        assert!(
            dump.contains(needle),
            "{scenario}: banner missing in {dump}"
        );
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nologin_account_exec_returns_server_message_and_nonzero_exit() {
    let mut t = login(
        "authpolicy-nologin",
        "nologinuser",
        password(PASS_REF),
        &NoPrompter,
        &ConnectLog::new(),
    )
    .await
    .unwrap();
    let out = step("execute(echo ok)", t.execute("echo ok"))
        .await
        .unwrap();
    let text =
        String::from_utf8_lossy(&[out.stdout.clone(), out.stderr.clone()].concat()).to_lowercase();
    assert!(
        text.contains("not available"),
        "server message missing: {text:?}"
    );
    assert!(
        matches!(out.exit_code, Some(c) if c != 0),
        "exit code {:?}",
        out.exit_code
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nologin_account_shell_ends_with_session_closed() {
    let mut t = login(
        "authpolicy-nologin",
        "nologinuser",
        password(PASS_REF),
        &NoPrompter,
        &ConnectLog::new(),
    )
    .await
    .unwrap();
    let mut shell = step(
        "open_shell()",
        t.open_shell(ssh_manager_core::ssh::PtySize { rows: 24, cols: 80 }),
    )
    .await
    .unwrap();
    // Read until the session ends (an error or an empty read); no hang.
    step("shell ends", async {
        loop {
            match shell.read().await {
                Ok(data) if !data.is_empty() => continue,
                _ => break,
            }
        }
    })
    .await;
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn locked_account_with_key_is_authentication_failed() {
    let t = login(
        "authpolicy-locked",
        "lockeduser",
        key(),
        &NoPrompter,
        &ConnectLog::new(),
    )
    .await;
    assert!(
        matches!(t, Err(SshError::AuthenticationFailed)),
        "expected AuthenticationFailed, got {:?}",
        t.err()
    );
}
