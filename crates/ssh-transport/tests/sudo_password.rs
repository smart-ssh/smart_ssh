//! Issue #1191: the sudo password path of the elevated file browser
//! (`open_sftp_via_exec_with_sudo_password`) against the in-process server,
//! which simulates `sudo -S -k -p '<marker>'`.
#![expect(clippy::unwrap_used, reason = "integration test")]
#![allow(
    dead_code,
    reason = "shared fixtures; this file uses only part of them"
)]
mod fixtures;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use secrecy::SecretString;
use ssh_manager_core::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore,
};
use ssh_manager_core::ssh::{
    ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, KeyFileContent, KeyFileError,
    KeyFileFacts, KeyFileReader, SshError, SshTransport, SudoPromptError,
};
use ssh_transport::ConnectOutcome;

use fixtures::connect_plain::connect_plain;
use fixtures::test_server::{sftp_local_path, RunningTestServer, TEST_PASSWORD, TEST_USERNAME};

struct NoKeyFiles;
impl KeyFileReader for NoKeyFiles {
    fn read(&self, _p: &str, _e: bool) -> Result<KeyFileContent, KeyFileError> {
        unreachable!()
    }
    fn inspect(&self, _p: &str) -> KeyFileFacts {
        unreachable!()
    }
}

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

#[derive(Default)]
struct HostKeys(Mutex<HashMap<(String, u16), Vec<u8>>>);
impl HostKeyStore for HostKeys {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        match self.0.lock().unwrap().get(&(host.to_string(), port)) {
            Some(k) if k.as_slice() == key => HostKeyDecision::Trusted,
            _ => HostKeyDecision::Unknown {
                fingerprint: String::new(),
            },
        }
    }
    fn trust(&self, _h: &str, _p: u16, _k: &[u8]) -> Result<(), SshError> {
        Ok(())
    }
}

async fn connect_trusted(server: &RunningTestServer) -> Box<dyn SshTransport> {
    let port = server.addr.port();
    let host_keys = HostKeys::default();
    host_keys.0.lock().unwrap().insert(
        ("127.0.0.1".to_string(), port),
        server.host_public_key.clone(),
    );
    let host_keys: Arc<dyn HostKeyStore> = Arc::new(host_keys);
    let target = ConnectionTarget {
        hops: vec![Hop {
            host: "127.0.0.1".to_string(),
            port,
            username: TEST_USERNAME.to_string(),
            auth: AuthMethod::Password {
                credential_ref: CredentialRef::new("pw"),
            },
        }],
    };
    match connect_plain(&target, &Credentials, &NoKeyFiles, host_keys)
        .await
        .expect("connect")
    {
        ConnectOutcome::Connected(t) => t,
        ConnectOutcome::PendingHostKeyConfirmation { .. } => panic!("host key was trusted"),
    }
}

fn sudo_password(value: &str) -> secrecy::SecretString {
    secrecy::SecretString::from(value.to_string())
}

const MARKER: &str = "smart-ssh-sudo-0123456789abcdef:";

fn pw_command(dir: &str) -> String {
    format!("sudo -S -k -p '{MARKER}' /{dir}/sftp-server")
}

/// Issue #1191: the password is written after the marker, exactly once, and
/// SFTP then runs on the same channel.
#[tokio::test]
async fn test_sudo_password_path_writes_password_once_after_marker() {
    let server = RunningTestServer::start().await;
    std::fs::create_dir(sftp_local_path(&server, "/elevated")).unwrap();
    std::fs::write(
        sftp_local_path(&server, "/elevated/nur-erhoeht.txt"),
        b"root",
    )
    .unwrap();
    let mut transport = connect_trusted(&server).await;

    let mut sftp = transport
        .open_sftp_via_exec_with_sudo_password(&pw_command("pw"), MARKER, &sudo_password("secret"))
        .await
        .map_err(|e| format!("{e:?}"))
        .expect("the correct password opens the channel");
    let names: Vec<String> = sftp
        .list_dir("/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, vec!["nur-erhoeht.txt".to_string()]);
    assert_eq!(server.sudo_stdin.lock().unwrap().as_slice(), b"secret\n");
}

/// Issue #1191: no marker → the channel is closed and not one password byte
/// reaches the server.
#[tokio::test]
async fn test_sudo_password_path_without_marker_writes_nothing() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;

    let err = transport
        .open_sftp_via_exec_with_sudo_password(
            &pw_command("noprompt"),
            MARKER,
            &sudo_password("secret"),
        )
        .await
        .err()
        .expect("no prompt means failure");
    assert!(matches!(err, SudoPromptError::NoPrompt), "{err:?}");
    assert!(server.sudo_stdin.lock().unwrap().is_empty());
}

/// Issue #1191: a second marker means the password was rejected.
#[tokio::test]
async fn test_sudo_password_path_second_marker_is_wrong_password() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;

    let err = transport
        .open_sftp_via_exec_with_sudo_password(&pw_command("pw"), MARKER, &sudo_password("nope"))
        .await
        .err()
        .expect("wrong password means failure");
    assert!(matches!(err, SudoPromptError::WrongPassword), "{err:?}");
    // No retry with the same password.
    assert_eq!(server.sudo_stdin.lock().unwrap().as_slice(), b"nope\n");
}

/// Issue #1191: "not allowed" on stderr before any prompt.
#[tokio::test]
async fn test_sudo_password_path_not_allowed() {
    let server = RunningTestServer::start().await;
    let mut transport = connect_trusted(&server).await;

    let err = transport
        .open_sftp_via_exec_with_sudo_password(
            &pw_command("notallowed"),
            MARKER,
            &sudo_password("secret"),
        )
        .await
        .err()
        .expect("not allowed means failure");
    assert!(matches!(err, SudoPromptError::NotAllowed(_)), "{err:?}");
    assert!(server.sudo_stdin.lock().unwrap().is_empty());
}
