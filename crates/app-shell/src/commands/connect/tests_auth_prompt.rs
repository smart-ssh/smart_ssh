//! Issue #1185: the login prompt of an interactive connect against the
//! in-process SSH test server, answered through the same registry the
//! `answer_auth_prompt` / `cancel_auth_prompt` commands use.
#![expect(clippy::expect_used, reason = "test code")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_logic::auth_prompter::{
    resolve_auth_prompt, AuthPromptAnswers, AuthPromptRegistry, AuthPromptResponse,
    EventAuthPrompter,
};
use app_logic::events::EventEmitter;
use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    ConnectLog, ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, KeyFileContent, KeyFileError,
    KeyFileFacts, KeyFileReader, SshError,
};
use ssh_transport::{ConnectLimits, ConnectOutcome};

use super::tests_flow::two_hop_fixture as fixture;
use fixture::{AuthScenario, RunningTestServer, TEST_CODE, TEST_PASSWORD, TEST_USERNAME};

struct Credentials;
impl CredentialStore for Credentials {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        if r.as_str() == "pw" {
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

struct NoKeys;
impl KeyFileReader for NoKeys {
    fn read(&self, _p: &str, _e: bool) -> Result<KeyFileContent, KeyFileError> {
        unreachable!("password login")
    }
    fn inspect(&self, _p: &str) -> KeyFileFacts {
        unreachable!("password login")
    }
}

struct Trusted(Vec<(u16, Vec<u8>)>);
impl HostKeyStore for Trusted {
    fn check(&self, _host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        if self.0.iter().any(|(p, k)| *p == port && k == key) {
            HostKeyDecision::Trusted
        } else {
            HostKeyDecision::Unknown {
                fingerprint: String::new(),
            }
        }
    }
    fn trust(&self, _h: &str, _p: u16, _k: &[u8]) -> Result<(), SshError> {
        Ok(())
    }
}

/// Plays the frontend: answers or cancels each `auth-prompt-needed`.
struct Frontend {
    registry: Arc<AuthPromptRegistry>,
    /// Answers per prompt, in order; running out cancels.
    script: Mutex<Vec<Vec<String>>>,
    events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl EventEmitter for Frontend {
    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        self.events
            .lock()
            .unwrap()
            .push((event.to_string(), payload.clone()));
        if event == "auth-prompt-needed" {
            let id = payload["promptId"].as_str().unwrap().to_string();
            let mut script = self.script.lock().unwrap();
            let response = if script.is_empty() {
                AuthPromptResponse::Cancel
            } else {
                AuthPromptResponse::Answers(AuthPromptAnswers::new(script.remove(0)))
            };
            drop(script);
            let registry = self.registry.clone();
            tokio::spawn(async move {
                resolve_auth_prompt(&registry, &id, response).unwrap();
            });
        }
    }
}

fn hop(server: &RunningTestServer) -> Hop {
    Hop {
        host: "127.0.0.1".to_string(),
        port: server.addr.port(),
        username: TEST_USERNAME.to_string(),
        auth: AuthMethod::Password {
            credential_ref: CredentialRef::new("pw"),
        },
    }
}

async fn connect(
    servers: &[&RunningTestServer],
    answers: Vec<Vec<String>>,
) -> (Result<(), SshError>, Arc<Frontend>) {
    let registry = Arc::new(AuthPromptRegistry::new());
    let frontend = Arc::new(Frontend {
        registry: registry.clone(),
        script: Mutex::new(answers),
        events: Mutex::default(),
    });
    let prompter = EventAuthPrompter::new(registry, frontend.clone());
    let target = ConnectionTarget {
        hops: servers.iter().map(|s| hop(s)).collect(),
    };
    let keys = Arc::new(Trusted(
        servers
            .iter()
            .map(|s| (s.addr.port(), s.host_public_key.clone()))
            .collect(),
    ));
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        ssh_transport::connect_with_limits(
            &target,
            &Credentials,
            &NoKeys,
            keys,
            &prompter,
            &ConnectLog::new(),
            ConnectLimits::DEFAULT,
        ),
    )
    .await
    .expect("login must not hang");
    let result = match result {
        Ok(ConnectOutcome::Connected(_)) => Ok(()),
        Ok(ConnectOutcome::PendingHostKeyConfirmation { .. }) => panic!("host key was trusted"),
        Err(e) => Err(e),
    };
    (result, frontend)
}

fn needed(frontend: &Frontend) -> Vec<serde_json::Value> {
    frontend
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "auth-prompt-needed")
        .map(|(_, payload)| payload.clone())
        .collect()
}

/// A password prompt (hop 1, password required again after the first step)
/// and a code prompt (hop 2): the dialog opens twice, names its hop, masks
/// the password and shows the code; after the answers the session opens.
#[tokio::test]
async fn test_password_and_code_prompt_open_two_dialogs_and_connect() {
    let first =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveThenPassword).await;
    let second =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveTwoRounds).await;
    let (result, frontend) = connect(
        &[&first, &second],
        vec![vec![TEST_PASSWORD.to_string()], vec![TEST_CODE.to_string()]],
    )
    .await;
    result.unwrap();
    let prompts = needed(&frontend);
    assert_eq!(prompts.len(), 2);
    assert_eq!(prompts[0]["kind"], "password");
    assert_eq!(
        prompts[0]["hop"],
        format!("{TEST_USERNAME}@127.0.0.1:{}", first.addr.port())
    );
    assert_eq!(prompts[1]["kind"], "keyboard_interactive");
    assert_eq!(
        prompts[1]["hop"],
        format!("{TEST_USERNAME}@127.0.0.1:{}", second.addr.port())
    );
    assert_eq!(prompts[1]["prompts"][0]["text"], "Verification code: ");
    assert_eq!(prompts[1]["prompts"][0]["echo"], true);
    // No answer travels in an event.
    let all = format!("{:?}", frontend.events.lock().unwrap());
    assert!(!all.contains(TEST_CODE) && !all.contains(TEST_PASSWORD));
}

/// Code prompt and password prompt each open exactly one dialog.
#[tokio::test]
async fn test_single_prompt_logins_each_open_one_dialog() {
    let server =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveTwice).await;
    let (result, frontend) = connect(&[&server], vec![vec![TEST_CODE.to_string()]]).await;
    result.unwrap();
    let prompts = needed(&frontend);
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0]["prompts"][0]["echo"], true);
    // Echo-off: the password round of a scenario without a stored answer.
    let server =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveThenPassword).await;
    let (result, frontend) = connect(&[&server], vec![vec![TEST_PASSWORD.to_string()]]).await;
    result.unwrap();
    assert_eq!(needed(&frontend)[0]["kind"], "password");
}

/// Cancel fails the connect with the keyboard-interactive error and asks no
/// further question.
#[tokio::test]
async fn test_cancel_fails_with_keyboard_interactive_error_and_no_second_dialog() {
    let server =
        RunningTestServer::start_with_scenario(AuthScenario::KeyboardInteractiveTwice).await;
    let (result, frontend) = connect(&[&server], Vec::new()).await;
    let err = result.unwrap_err();
    assert!(err.to_string().contains("keyboard-interactive"), "{err}");
    assert_eq!(needed(&frontend).len(), 1);
    let events = frontend.events.lock().unwrap();
    let ended = events
        .iter()
        .find(|(n, _)| n == "auth-prompt-ended")
        .unwrap();
    assert_eq!(ended.1["reason"], "cancelled");
}
