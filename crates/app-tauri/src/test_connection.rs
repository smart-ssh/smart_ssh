//! `test_connection` (Spec 0008, Abschnitt 7) — prüft Erreichbarkeit/
//! Zugangsdaten eines (ggf. noch nicht gespeicherten) Servers, ohne
//! irgendetwas zu persistieren.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use secrecy::SecretString;

use ssh_manager_core::profiles::{AuthMethod, CredentialRef, CredentialStore, ProfileStore};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{
    resolve_connection_target, ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, SshError,
};
use ssh_transport::ConnectOutcome;

use crate::dto::{AuthMethodInput, ServerInput, TestConnectionResult};
use crate::ephemeral_credentials::EphemeralCredentialStore;
use crate::error::{CommandError, CommandResult};

const TEST_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Kapselt `ssh_transport::connect()` hinter einem Trait, rein damit
/// `test_connection`s Logik (Ephemeral-Credential-Aufbau, Hop-Kette,
/// Timeout, `SshError`/`ConnectOutcome` → `TestConnectionResult`-Mapping)
/// gegen einen `MockConnector` testbar ist, ohne echtes Netzwerk zu
/// brauchen (Aufgabenstellung Teil 1, Punkt 6: "gegen MockSshTransport ...
/// für alle TestConnectionResult-Varianten"). `ssh_transport::connect` ist
/// eine freie Funktion, kein Trait-Objekt (anders als `SshTransport`
/// selbst, das erst NACH einem erfolgreichen Verbindungsaufbau greift) —
/// ohne diese Abstraktion ließe sich der Verbindungsversuch selbst gar
/// nicht mocken.
#[async_trait]
pub trait Connector: Send + Sync {
    async fn connect(
        &self,
        target: &ConnectionTarget,
        credentials: &(dyn CredentialStore + Send + Sync),
        host_keys: Arc<dyn HostKeyStore>,
    ) -> Result<ConnectOutcome, SshError>;
}

pub struct RealConnector;

#[async_trait]
impl Connector for RealConnector {
    async fn connect(
        &self,
        target: &ConnectionTarget,
        credentials: &(dyn CredentialStore + Send + Sync),
        host_keys: Arc<dyn HostKeyStore>,
    ) -> Result<ConnectOutcome, SshError> {
        ssh_transport::connect(target, credentials, host_keys).await
    }
}

/// Spec 0008, Abschnitt 7. `existing_server_id` ist die in der Spec-Skizze
/// fehlende, aber notwendige Ergänzung: `test_connection(input:
/// ServerInput)` allein kann nicht wissen, **welcher** gespeicherte Server
/// gemeint ist, wenn ein Secret-Feld leer ("unverändert lassen") ist — die
/// Spec selbst verlangt aber genau dieses Verhalten ("wird für den Test
/// das bereits gespeicherte Credential des existierenden Servers
/// herangezogen"). Siehe ADR-Vorschlag am Ende der Aufgabe.
pub async fn test_connection(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    host_key_store: Arc<dyn HostKeyStore>,
    connector: &dyn Connector,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
) -> CommandResult<TestConnectionResult> {
    test_connection_with_timeout(
        profile_store,
        real_credential_store,
        host_key_store,
        connector,
        input,
        existing_server_id,
        TEST_CONNECTION_TIMEOUT,
    )
    .await
}

/// Testbare Variante mit injizierbarem Timeout — die echten 10 Sekunden
/// aus der Spec wären in einem Unit-Test schlicht zu langsam.
async fn test_connection_with_timeout(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    host_key_store: Arc<dyn HostKeyStore>,
    connector: &dyn Connector,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
    timeout: Duration,
) -> CommandResult<TestConnectionResult> {
    let existing_auth = match existing_server_id {
        Some(id) => Some(profile_store.get_server(&id).await?.auth),
        None => None,
    };

    let ephemeral = EphemeralCredentialStore::new();
    let final_auth = resolve_final_hop_auth(
        &ephemeral,
        real_credential_store,
        input.auth,
        existing_auth.as_ref(),
    )?;

    let mut hops = Vec::new();
    if let Some(jump_id) = input.jump_host {
        // Spec 0008 Abschnitt 7: "bereits gespeicherte Zwischen-Hops werden
        // regulär über ProfileStore aufgelöst" — derselbe Weg wie beim
        // echten `connect()` (Spec 0007), nur dass danach noch der frische
        // letzte Hop angehängt wird statt die Kette dort enden zu lassen.
        let jump_server = profile_store.get_server(&jump_id).await?;
        let jump_target = resolve_connection_target(&jump_server, profile_store).await?;
        hops.extend(jump_target.hops);
    }
    hops.push(Hop {
        host: input.host,
        port: input.port,
        username: input.username,
        auth: final_auth,
    });
    let target = ConnectionTarget { hops };

    let attempt = connector.connect(&target, &ephemeral, host_key_store);
    let outcome = match tokio::time::timeout(timeout, attempt).await {
        Err(_elapsed) => return Ok(TestConnectionResult::Timeout),
        Ok(result) => result,
    };

    Ok(match outcome {
        Ok(ConnectOutcome::Connected(mut transport)) => {
            // Nur der Auth-Handshake wird geprüft — kein `execute()`, kein
            // Session-Eintrag (Spec Abschnitt 7). Verbindung sofort wieder
            // schließen.
            let _ = transport.disconnect().await;
            TestConnectionResult::Success
        }
        Ok(ConnectOutcome::PendingHostKeyConfirmation {
            host,
            port,
            raw_key,
            decision,
        }) => match decision {
            HostKeyDecision::Unknown { fingerprint } => TestConnectionResult::HostKeyUnknown {
                host,
                port,
                raw_key,
                fingerprint,
            },
            HostKeyDecision::Mismatch {
                expected_fingerprint,
                actual_fingerprint,
            } => TestConnectionResult::HostKeyMismatch {
                host,
                port,
                raw_key,
                expected_fingerprint,
                actual_fingerprint,
            },
            HostKeyDecision::Trusted => {
                unreachable!("PendingHostKeyConfirmation wird nur für Unknown/Mismatch gebaut")
            }
        },
        Err(SshError::AuthenticationFailed) => TestConnectionResult::AuthenticationFailed,
        Err(SshError::Timeout) => TestConnectionResult::Timeout,
        Err(other) => TestConnectionResult::NetworkError {
            message: other.to_string(),
        },
    })
}

/// Baut das `AuthMethod` für den frischen letzten Hop, befüllt dabei
/// `ephemeral` mit den benötigten Secrets — entweder aus `input` selbst
/// oder (bei leerem Feld) aus dem bereits gespeicherten Credential von
/// `existing` (s. Modul-Doc).
fn resolve_final_hop_auth(
    ephemeral: &EphemeralCredentialStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    input: AuthMethodInput,
    existing: Option<&AuthMethod>,
) -> CommandResult<AuthMethod> {
    match input {
        AuthMethodInput::Password { value } => {
            let secret = resolve_secret(value, existing, real_credential_store, |a| match a {
                AuthMethod::Password { credential_ref } => Some(credential_ref),
                _ => None,
            })?;
            let r = CredentialRef::new("test:password");
            ephemeral.insert(&r, secret);
            Ok(AuthMethod::Password { credential_ref: r })
        }
        AuthMethodInput::PrivateKey {
            key_content,
            passphrase,
        } => {
            let key_secret =
                resolve_secret(key_content, existing, real_credential_store, |a| match a {
                    AuthMethod::PrivateKey { credential_ref, .. } => Some(credential_ref),
                    _ => None,
                })?;
            let key_ref = CredentialRef::new("test:private_key");
            ephemeral.insert(&key_ref, key_secret);

            let passphrase_ref = match passphrase {
                Some(p) => {
                    let r = CredentialRef::new("test:passphrase");
                    ephemeral.insert(&r, SecretString::from(p));
                    Some(r)
                }
                None => match existing {
                    Some(AuthMethod::PrivateKey {
                        passphrase_ref: Some(existing_ref),
                        ..
                    }) => {
                        let secret = real_credential_store.get(existing_ref)?;
                        let r = CredentialRef::new("test:passphrase");
                        ephemeral.insert(&r, secret);
                        Some(r)
                    }
                    _ => None,
                },
            };
            Ok(AuthMethod::PrivateKey {
                credential_ref: key_ref,
                passphrase_ref,
            })
        }
        AuthMethodInput::Agent => Ok(AuthMethod::Agent),
        AuthMethodInput::Certificate {
            cert_content,
            key_content,
        } => {
            let cert_secret =
                resolve_secret(cert_content, existing, real_credential_store, |a| match a {
                    AuthMethod::Certificate { cert_ref, .. } => Some(cert_ref),
                    _ => None,
                })?;
            let cert_ref = CredentialRef::new("test:certificate");
            ephemeral.insert(&cert_ref, cert_secret);

            let key_secret =
                resolve_secret(key_content, existing, real_credential_store, |a| match a {
                    AuthMethod::Certificate { key_ref, .. } => Some(key_ref),
                    _ => None,
                })?;
            let key_ref = CredentialRef::new("test:certificate_key");
            ephemeral.insert(&key_ref, key_secret);

            Ok(AuthMethod::Certificate { cert_ref, key_ref })
        }
    }
}

fn resolve_secret(
    provided: Option<String>,
    existing: Option<&AuthMethod>,
    real_store: &(dyn CredentialStore + Send + Sync),
    extract_ref: impl Fn(&AuthMethod) -> Option<&CredentialRef>,
) -> CommandResult<SecretString> {
    if let Some(value) = provided {
        return Ok(SecretString::from(value));
    }
    let existing_ref = existing.and_then(extract_ref).ok_or_else(|| {
        CommandError::from(
            "Secret erforderlich (kein bestehender Server zum Wiederverwenden gefunden)",
        )
    })?;
    Ok(real_store.get(existing_ref)?)
}

#[cfg(test)]
mod tests {
    use ssh_manager_core::ssh::{CommandOutput, HostKeyDecision, InteractiveShell, PtySize};

    use super::*;
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};

    struct NoOpHostKeyStore;
    impl HostKeyStore for NoOpHostKeyStore {
        fn check(&self, _host: &str, _port: u16, _key: &[u8]) -> HostKeyDecision {
            HostKeyDecision::Trusted
        }
        fn trust(&self, _host: &str, _port: u16, _key: &[u8]) -> Result<(), SshError> {
            Ok(())
        }
    }

    struct StubSshTransport;
    #[async_trait]
    impl ssh_manager_core::ssh::SshTransport for StubSshTransport {
        async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
            Err(SshError::ChannelError(
                "in diesem Test nicht unterstützt".to_string(),
            ))
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
            Err(SshError::ChannelError(
                "in diesem Test nicht unterstützt".to_string(),
            ))
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    enum MockOutcome {
        Success,
        HostKeyUnknown,
        HostKeyMismatch,
        Error(SshError),
        /// Schläft länger als jeder in den Tests verwendete Timeout —
        /// simuliert eine hängende Verbindung, ohne echtes Netzwerk.
        Hang,
    }

    struct MockConnector(MockOutcome);

    #[async_trait]
    impl Connector for MockConnector {
        async fn connect(
            &self,
            _target: &ConnectionTarget,
            _credentials: &(dyn CredentialStore + Send + Sync),
            _host_keys: Arc<dyn HostKeyStore>,
        ) -> Result<ConnectOutcome, SshError> {
            match &self.0 {
                MockOutcome::Success => Ok(ConnectOutcome::Connected(Box::new(StubSshTransport))),
                MockOutcome::HostKeyUnknown => Ok(ConnectOutcome::PendingHostKeyConfirmation {
                    host: "example.invalid".to_string(),
                    port: 22,
                    raw_key: b"raw-key".to_vec(),
                    decision: HostKeyDecision::Unknown {
                        fingerprint: "SHA256:unknown".to_string(),
                    },
                }),
                MockOutcome::HostKeyMismatch => Ok(ConnectOutcome::PendingHostKeyConfirmation {
                    host: "example.invalid".to_string(),
                    port: 22,
                    raw_key: b"raw-key".to_vec(),
                    decision: HostKeyDecision::Mismatch {
                        expected_fingerprint: "SHA256:old".to_string(),
                        actual_fingerprint: "SHA256:new".to_string(),
                    },
                }),
                MockOutcome::Error(e) => Err(e.clone()),
                MockOutcome::Hang => {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    unreachable!("Timeout-Test sollte längst abgebrochen haben")
                }
            }
        }
    }

    fn password_input() -> ServerInput {
        ServerInput {
            name: "test".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethodInput::Password {
                value: Some("hunter2".to_string()),
            },
            jump_host: None,
        }
    }

    async fn run(outcome: MockOutcome, input: ServerInput) -> TestConnectionResult {
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();
        test_connection_with_timeout(
            &profile_store,
            &credential_store,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(outcome),
            input,
            None,
            Duration::from_millis(200),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_success() {
        let result = run(MockOutcome::Success, password_input()).await;
        assert!(matches!(result, TestConnectionResult::Success));
    }

    #[tokio::test]
    async fn test_authentication_failed() {
        let result = run(
            MockOutcome::Error(SshError::AuthenticationFailed),
            password_input(),
        )
        .await;
        assert!(matches!(result, TestConnectionResult::AuthenticationFailed));
    }

    #[tokio::test]
    async fn test_host_key_unknown() {
        let result = run(MockOutcome::HostKeyUnknown, password_input()).await;
        assert!(matches!(
            result,
            TestConnectionResult::HostKeyUnknown { .. }
        ));
    }

    #[tokio::test]
    async fn test_host_key_mismatch() {
        let result = run(MockOutcome::HostKeyMismatch, password_input()).await;
        assert!(matches!(
            result,
            TestConnectionResult::HostKeyMismatch { .. }
        ));
    }

    #[tokio::test]
    async fn test_network_error() {
        let result = run(
            MockOutcome::Error(SshError::ConnectionFailed("refused".to_string())),
            password_input(),
        )
        .await;
        assert!(matches!(result, TestConnectionResult::NetworkError { .. }));
    }

    #[tokio::test]
    async fn test_timeout() {
        let result = run(MockOutcome::Hang, password_input()).await;
        assert!(matches!(result, TestConnectionResult::Timeout));
    }

    #[tokio::test]
    async fn test_empty_secret_without_existing_server_is_a_hard_error_not_a_result() {
        let input = ServerInput {
            auth: AuthMethodInput::Password { value: None },
            ..password_input()
        };
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(MockOutcome::Success),
            input,
            None,
            Duration::from_millis(200),
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_empty_secret_with_existing_server_falls_back_to_stored_credential() {
        use chrono::Utc;
        use ssh_manager_core::profiles::{CredentialRef, Server};

        let existing_id = ServerId::new();
        let password_ref = CredentialRef::new("server:existing:password");
        let now = Utc::now();
        let existing_server = Server {
            id: existing_id,
            name: "existing".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethod::Password {
                credential_ref: password_ref.clone(),
            },
            notes: String::new(),
            jump_host: None,
            created_at: now,
            updated_at: now,
        };
        let profile_store = InMemoryProfileStore::new().with_server(existing_server);
        let credential_store =
            InMemoryCredentialStore::new().with_secret(&password_ref, "stored-secret");

        let input = ServerInput {
            auth: AuthMethodInput::Password { value: None },
            ..password_input()
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(MockOutcome::Success),
            input,
            Some(existing_id),
            Duration::from_millis(200),
        )
        .await
        .unwrap();

        assert!(matches!(result, TestConnectionResult::Success));
    }
}
