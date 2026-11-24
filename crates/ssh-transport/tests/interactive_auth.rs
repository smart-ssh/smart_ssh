//! Issue #1184 (Spec 0005, "Interactive and multi-step login"): the
//! transport against an in-process server with `keyboard-interactive` and
//! multi-step logins, answered by a scripted prompter.
#![expect(clippy::unwrap_used, reason = "integration test")]
#![allow(
    dead_code,
    reason = "shared fixtures; this file uses only part of them"
)]
mod fixtures;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    AuthPromptKind, AuthPromptRequest, AuthPrompter, ConnectLog, ConnectStep, ConnectionTarget,
    Hop, HostKeyDecision, HostKeyStore, KeyFileContent, KeyFileError, KeyFileFacts, KeyFileReader,
    NoPrompter, PromptCancelled, SshError,
};
use ssh_transport::{ConnectLimits, ConnectOutcome};

use fixtures::test_server::{
    AuthScenario, RunningTestServer, TEST_CODE, TEST_PASSWORD, TEST_USERNAME,
};

const PASSWORD_CREDENTIAL: &str = "pw";

struct Credentials;

impl CredentialStore for Credentials {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        if r.as_str() == PASSWORD_CREDENTIAL {
            Ok(SecretString::from(TEST_PASSWORD.to_string()))
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

struct KeyFile(String);

impl KeyFileReader for KeyFile {
    fn read(&self, _path: &str, _enforce: bool) -> Result<KeyFileContent, KeyFileError> {
        Ok(KeyFileContent {
            key: SecretString::from(self.0.clone()),
            encrypted: false,
        })
    }
    fn inspect(&self, _path: &str) -> KeyFileFacts {
        unreachable!("login uses `read`")
    }
}

#[derive(Default)]
struct HostKeys(Mutex<HashMap<(String, u16), Vec<u8>>>);

impl HostKeys {
    fn trusting(servers: &[&RunningTestServer]) -> Arc<dyn HostKeyStore> {
        let store = Self::default();
        for s in servers {
            store.0.lock().unwrap().insert(
                ("127.0.0.1".to_string(), s.addr.port()),
                s.host_public_key.clone(),
            );
        }
        Arc::new(store)
    }
}

impl HostKeyStore for HostKeys {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        match self.0.lock().unwrap().get(&(host.to_string(), port)) {
            Some(k) if k.as_slice() == key => HostKeyDecision::Trusted,
            _ => HostKeyDecision::Unknown {
                fingerprint: String::new(),
            },
        }
    }
    fn trust(&self, _host: &str, _port: u16, _key: &[u8]) -> Result<(), SshError> {
        Ok(())
    }
}

/// Answers from a script, one entry per request; records every request.
/// An empty script cancels.
#[derive(Default)]
struct ScriptedPrompter {
    script: Mutex<VecDeque<Vec<&'static str>>>,
    requests: Mutex<Vec<AuthPromptRequest>>,
    delay: Duration,
}

impl ScriptedPrompter {
    fn new(script: &[&[&'static str]]) -> Self {
        Self {
            script: Mutex::new(script.iter().map(|s| s.to_vec()).collect()),
            ..Default::default()
        }
    }
    fn requests(&self) -> Vec<AuthPromptRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl AuthPrompter for ScriptedPrompter {
    async fn prompt(
        &self,
        request: AuthPromptRequest,
    ) -> Result<Vec<SecretString>, PromptCancelled> {
        self.requests.lock().unwrap().push(request);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        let answers = self.script.lock().unwrap().pop_front();
        answers
            .map(|a| {
                a.into_iter()
                    .map(|s| SecretString::from(s.to_string()))
                    .collect()
            })
            .ok_or(PromptCancelled)
    }
}

/// Answers every request with one more answer than asked for.
struct Endless;

#[async_trait]
impl AuthPrompter for Endless {
    async fn prompt(
        &self,
        request: AuthPromptRequest,
    ) -> Result<Vec<SecretString>, PromptCancelled> {
        let n = match request.kind {
            AuthPromptKind::KeyboardInteractive { prompts, .. } => prompts.len(),
            AuthPromptKind::Password => 1,
        };
        Ok(vec![SecretString::from("x".to_string()); n])
    }
}

fn hop(server: &RunningTestServer, auth: AuthMethod) -> Hop {
    Hop {
        host: "127.0.0.1".to_string(),
        port: server.addr.port(),
        username: TEST_USERNAME.to_string(),
        auth,
    }
}

fn password_auth() -> AuthMethod {
    AuthMethod::Password {
        credential_ref: CredentialRef::new(PASSWORD_CREDENTIAL),
    }
}

fn key_auth() -> AuthMethod {
    AuthMethod::IdentityFile {
        path: "/key".to_string(),
        passphrase_ref: None,
    }
}

fn key_files() -> KeyFile {
    use russh::keys::ssh_key::LineEnding;
    use russh::keys::{Algorithm, PrivateKey};
    KeyFile(
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .unwrap()
            .to_openssh(LineEnding::LF)
            .unwrap()
            .to_string(),
    )
}

async fn login(
    hops: Vec<Hop>,
    host_keys: Arc<dyn HostKeyStore>,
    prompter: &dyn AuthPrompter,
    log: &ConnectLog,
    limits: ConnectLimits,
) -> Result<(), SshError> {
    let target = ConnectionTarget { hops };
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        ssh_transport::connect_with_limits(
            &target,
            &Credentials,
            &key_files(),
            host_keys,
            prompter,
            log,
            limits,
        ),
    )
    .await
    .expect("login must not hang");
    match result {
        Ok(ConnectOutcome::Connected(_)) => Ok(()),
        Ok(ConnectOutcome::PendingHostKeyConfirmation { .. }) => panic!("host key was trusted"),
        Err(e) => Err(e),
    }
}

async fn login_one(
    scenario: AuthScenario,
    auth: AuthMethod,
    prompter: &dyn AuthPrompter,
) -> (Result<(), SshError>, ConnectLog) {
    let server = RunningTestServer::start_with_scenario(scenario).await;
    let log = ConnectLog::new();
    let result = login(
        vec![hop(&server, auth)],
        HostKeys::trusting(&[&server]),
        prompter,
        &log,
        ConnectLimits::DEFAULT,
    )
    .await;
    (result, log)
}

fn kbd_prompts(request: &AuthPromptRequest) -> Vec<(String, bool)> {
    match &request.kind {
        AuthPromptKind::KeyboardInteractive { prompts, .. } => {
            prompts.iter().map(|p| (p.text.clone(), p.echo)).collect()
        }
        AuthPromptKind::Password => panic!("expected a keyboard-interactive request"),
    }
}

#[tokio::test]
async fn test_keyboard_interactive_only_with_prompter() {
    let prompter = ScriptedPrompter::new(&[&[TEST_PASSWORD]]);
    let (result, _) = login_one(
        AuthScenario::KeyboardInteractivePassword,
        key_auth(),
        &prompter,
    )
    .await;
    // A key profile is rejected first and the server offers only
    // keyboard-interactive: no silent switch, the error names the method.
    assert!(matches!(
        result,
        Err(SshError::AuthenticationMethodRequired { .. })
    ));
    assert!(prompter.requests().is_empty());
}

#[tokio::test]
async fn test_password_profile_answers_single_prompt_without_prompter() {
    let prompter = ScriptedPrompter::new(&[]);
    let (result, _) = login_one(
        AuthScenario::KeyboardInteractivePassword,
        password_auth(),
        &prompter,
    )
    .await;
    result.unwrap();
    assert!(prompter.requests().is_empty(), "stored password answers");
}

#[tokio::test]
async fn test_two_rounds_password_then_code() {
    let prompter = ScriptedPrompter::new(&[&[TEST_CODE]]);
    let server =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveTwoRounds).await;
    let log = ConnectLog::new();
    let h = hop(&server, password_auth());
    let label = format!("{}@127.0.0.1:{}", TEST_USERNAME, server.addr.port());
    login(
        vec![h],
        HostKeys::trusting(&[&server]),
        &prompter,
        &log,
        ConnectLimits::DEFAULT,
    )
    .await
    .unwrap();
    // The stored password answered round 1; the code went to the prompter.
    let requests = prompter.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].hop, label);
    assert_eq!(
        kbd_prompts(&requests[0]),
        vec![("Verification code: ".to_string(), true)]
    );
}

/// `keyboard-interactive,keyboard-interactive`: the second login is
/// answered by the prompter, which sees the hop label, texts and echo flags.
#[tokio::test]
async fn test_partial_success_then_keyboard_interactive_via_prompter() {
    let prompter = ScriptedPrompter::new(&[&[TEST_CODE]]);
    let server =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveTwice).await;
    let log = ConnectLog::new();
    login(
        vec![hop(&server, password_auth())],
        HostKeys::trusting(&[&server]),
        &prompter,
        &log,
        ConnectLimits::DEFAULT,
    )
    .await
    .unwrap();
    let requests = prompter.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].hop,
        format!("{}@127.0.0.1:{}", TEST_USERNAME, server.addr.port())
    );
    assert_eq!(
        kbd_prompts(&requests[0]),
        vec![("Verification code: ".to_string(), true)]
    );
}

/// `keyboard-interactive,password`: after the partial success the password
/// is asked through the prompter, not read from storage again.
#[tokio::test]
async fn test_partial_success_then_password_via_prompter() {
    let prompter = ScriptedPrompter::new(&[&[TEST_PASSWORD]]);
    let (result, _) = login_one(
        AuthScenario::KeyboardInteractiveThenPassword,
        password_auth(),
        &prompter,
    )
    .await;
    result.unwrap();
    let requests = prompter.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].kind, AuthPromptKind::Password);
}

#[tokio::test]
async fn test_cancelled_prompter_and_no_prompter_name_the_methods() {
    for prompter in [
        &ScriptedPrompter::new(&[]) as &dyn AuthPrompter,
        &NoPrompter as &dyn AuthPrompter,
    ] {
        let (result, _) = login_one(
            AuthScenario::KeyboardInteractiveTwice,
            password_auth(),
            prompter,
        )
        .await;
        let err = result.unwrap_err();
        assert_eq!(
            err,
            SshError::AuthenticationMethodRequired {
                methods: vec!["keyboard-interactive".to_string()]
            }
        );
        assert!(err.to_string().contains("keyboard-interactive"));
        assert_eq!(err.code(), SshError::AuthenticationFailed.code());
    }
}

#[tokio::test]
async fn test_empty_info_request_is_answered_without_asking() {
    let prompter = ScriptedPrompter::new(&[]);
    let (result, _) = login_one(
        AuthScenario::KeyboardInteractiveEmptyRound,
        password_auth(),
        &prompter,
    )
    .await;
    result.unwrap();
    assert!(prompter.requests().is_empty());
}

#[tokio::test]
async fn test_prompter_wait_does_not_consume_the_authentication_limit() {
    let mut prompter = ScriptedPrompter::new(&[&[TEST_CODE]]);
    // Well above the authentication limit below, so the test only passes while
    // the prompter wait is excluded from that limit; the limit itself leaves
    // ample room for the unpaused part of the login on a slow runner.
    prompter.delay = Duration::from_secs(6);
    let server =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveTwice).await;
    let limits = ConnectLimits {
        handshake: Duration::from_secs(10),
        authentication: Duration::from_secs(5),
    };
    let log = ConnectLog::new();
    login(
        vec![hop(&server, password_auth())],
        HostKeys::trusting(&[&server]),
        &prompter,
        &log,
        limits,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn test_more_than_ten_rounds_fail() {
    let (result, log) = login_one(
        AuthScenario::KeyboardInteractiveEndless,
        password_auth(),
        &Endless,
    )
    .await;
    assert_eq!(result.unwrap_err(), SshError::AuthenticationFailed);
    let rounds = log
        .snapshot()
        .iter()
        .filter(|r| matches!(r.step, ConnectStep::Authentication { round: Some(_), .. }))
        .count();
    assert_eq!(rounds, 10);
}

#[tokio::test]
async fn test_two_hop_chain_names_hop_two() {
    let first = RunningTestServer::start().await;
    let second =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveTwice).await;
    let prompter = ScriptedPrompter::new(&[&[TEST_CODE]]);
    let log = ConnectLog::new();
    login(
        vec![hop(&first, password_auth()), hop(&second, password_auth())],
        HostKeys::trusting(&[&first, &second]),
        &prompter,
        &log,
        ConnectLimits::DEFAULT,
    )
    .await
    .unwrap();
    let requests = prompter.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].hop,
        format!("{}@127.0.0.1:{}", TEST_USERNAME, second.addr.port())
    );
}

#[tokio::test]
async fn test_connect_log_has_prompt_texts_and_no_answers() {
    let prompter = ScriptedPrompter::new(&[&[TEST_CODE]]);
    let (result, log) = login_one(
        AuthScenario::KeyboardInteractiveTwoRounds,
        password_auth(),
        &prompter,
    )
    .await;
    result.unwrap();
    let dump = format!("{:?}", log.snapshot());
    assert!(dump.contains("Verification code: "));
    assert!(!dump.contains(TEST_PASSWORD), "password in log");
    assert!(!dump.contains(TEST_CODE), "code in log");
}

#[tokio::test]
async fn test_endless_partial_success_stops_after_three_steps() {
    let (result, log) = login_one(AuthScenario::PartialForever, password_auth(), &Endless).await;
    assert_eq!(result.unwrap_err(), SshError::AuthenticationFailed);
    let ki_logins = log
        .snapshot()
        .iter()
        .filter(|r| matches!(&r.step, ConnectStep::Authentication { round: Some(1), .. }))
        .count();
    // The stored-password login plus three follow-up steps.
    assert_eq!(ki_logins, 4);
}
