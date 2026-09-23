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
    resolve_connection_target, ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, KeyFileReader,
    SshError,
};
use ssh_transport::ConnectOutcome;

use crate::commands::SSH_CONNECT_TIMEOUT;
use crate::dto::{AuthMethodInput, ServerInput, TestConnectionResult};
use crate::ephemeral_credentials::EphemeralCredentialStore;
use credentials_keyring::KeychainAvailability;

use crate::error::{keychain_aware_credential_error, CommandError, CommandResult};

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
        key_files: &(dyn KeyFileReader + Send + Sync),
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
        key_files: &(dyn KeyFileReader + Send + Sync),
        host_keys: Arc<dyn HostKeyStore>,
    ) -> Result<ConnectOutcome, SshError> {
        ssh_transport::connect(target, credentials, key_files, host_keys).await
    }
}

/// Spec 0008, Abschnitt 7. `existing_server_id` ist die in der Spec-Skizze
/// fehlende, aber notwendige Ergänzung: `test_connection(input:
/// ServerInput)` allein kann nicht wissen, **welcher** gespeicherte Server
/// gemeint ist, wenn ein Secret-Feld leer ("unverändert lassen") ist — die
/// Spec selbst verlangt aber genau dieses Verhalten ("wird für den Test
/// das bereits gespeicherte Credential des existierenden Servers
/// herangezogen"). Siehe ADR-Vorschlag am Ende der Aufgabe.
#[allow(clippy::too_many_arguments)]
pub async fn test_connection(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    // Spec 0076, §4.2: reist neben `real_credential_store` mit — ein
    // Verbindungstest gegen einen Server mit Schlüsseldatei muss dieselbe
    // Datei lesen wie der echte Verbindungsaufbau.
    key_files: &(dyn KeyFileReader + Send + Sync),
    // Spec 0071, A13: nur für die Fehlerkennzeichnung durchgereicht (s.
    // `error::keychain_aware_credential_error`) — dieser Pfad liest bei
    // leerem Formularfeld das bereits gespeicherte Secret, und ohne
    // Schlüsselbund stünde sonst der englische Bibliothekstext im
    // Server-Formular.
    keychain: KeychainAvailability,
    host_key_store: Arc<dyn HostKeyStore>,
    connector: &dyn Connector,
    input: ServerInput,
    existing_server_id: Option<ServerId>,
) -> CommandResult<TestConnectionResult> {
    test_connection_with_timeout(
        profile_store,
        real_credential_store,
        key_files,
        keychain,
        host_key_store,
        connector,
        input,
        existing_server_id,
        SSH_CONNECT_TIMEOUT,
    )
    .await
}

/// Testbare Variante mit injizierbarem Timeout — die echten 10 Sekunden
/// aus der Spec wären in einem Unit-Test schlicht zu langsam.
/// `#[allow(clippy::too_many_arguments)]`: Spec 0071 ergänzt einen achten
/// Parameter (`keychain`). Ihn in eine Struct zu bündeln hieße, die
/// Signatur der öffentlichen [`test_connection`] mitzuziehen, ohne dass
/// diese Spec daran etwas fachlich ändert — dasselbe Muster wie an den
/// übrigen Stellen in dieser Crate.
#[allow(clippy::too_many_arguments)]
async fn test_connection_with_timeout(
    profile_store: &dyn ProfileStore,
    real_credential_store: &(dyn CredentialStore + Send + Sync),
    key_files: &(dyn KeyFileReader + Send + Sync),
    keychain: KeychainAvailability,
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
        keychain,
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

    let tiered = TieredCredentialStore {
        ephemeral: &ephemeral,
        real: real_credential_store,
    };

    let attempt = connector.connect(&target, &tiered, key_files, host_key_store);
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
            code: Some(other.code()),
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
    keychain: KeychainAvailability,
    input: AuthMethodInput,
    existing: Option<&AuthMethod>,
) -> CommandResult<AuthMethod> {
    match input {
        AuthMethodInput::Password { value } => {
            let secret =
                resolve_secret(
                    value,
                    existing,
                    real_credential_store,
                    keychain,
                    |a| match a {
                        AuthMethod::Password { credential_ref } => Some(credential_ref),
                        _ => None,
                    },
                )?;
            let r = CredentialRef::new("test:password");
            ephemeral.insert(&r, secret);
            Ok(AuthMethod::Password { credential_ref: r })
        }
        AuthMethodInput::PrivateKey {
            key_content,
            passphrase,
        } => {
            let key_secret = resolve_secret(
                key_content,
                existing,
                real_credential_store,
                keychain,
                |a| match a {
                    AuthMethod::PrivateKey { credential_ref, .. } => Some(credential_ref),
                    _ => None,
                },
            )?;
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
                        let secret = real_credential_store
                            .get(existing_ref)
                            .map_err(|err| keychain_aware_credential_error(err, keychain))?;
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
            let cert_secret = resolve_secret(
                cert_content,
                existing,
                real_credential_store,
                keychain,
                |a| match a {
                    AuthMethod::Certificate { cert_ref, .. } => Some(cert_ref),
                    _ => None,
                },
            )?;
            let cert_ref = CredentialRef::new("test:certificate");
            ephemeral.insert(&cert_ref, cert_secret);

            let key_secret = resolve_secret(
                key_content,
                existing,
                real_credential_store,
                keychain,
                |a| match a {
                    AuthMethod::Certificate { key_ref, .. } => Some(key_ref),
                    _ => None,
                },
            )?;
            let key_ref = CredentialRef::new("test:certificate_key");
            ephemeral.insert(&key_ref, key_secret);

            Ok(AuthMethod::Certificate { cert_ref, key_ref })
        }
        // Spec 0076: Der Pfad wandert unverändert in den Test-Hop — er ist
        // kein Secret und braucht keinen Ephemeral-Slot. Gelesen wird die
        // Datei erst im Verbindungsversuch selbst, über denselben
        // `KeyFileReader` wie beim echten Verbinden. Nur die Passphrase
        // folgt der gewohnten „leer = das Gespeicherte nehmen"-Regel.
        AuthMethodInput::IdentityFile { path, passphrase } => {
            let passphrase_ref = match passphrase {
                Some(p) if !p.trim().is_empty() => {
                    let r = CredentialRef::new("test:passphrase");
                    ephemeral.insert(&r, SecretString::from(p));
                    Some(r)
                }
                _ => match existing {
                    Some(AuthMethod::IdentityFile {
                        passphrase_ref: Some(existing_ref),
                        ..
                    }) => {
                        let secret = real_credential_store
                            .get(existing_ref)
                            .map_err(|err| keychain_aware_credential_error(err, keychain))?;
                        let r = CredentialRef::new("test:passphrase");
                        ephemeral.insert(&r, secret);
                        Some(r)
                    }
                    _ => None,
                },
            };
            Ok(AuthMethod::IdentityFile {
                path,
                passphrase_ref,
            })
        }
    }
}

struct TieredCredentialStore<'a> {
    ephemeral: &'a (dyn CredentialStore + Send + Sync),
    real: &'a (dyn CredentialStore + Send + Sync),
}

impl<'a> CredentialStore for TieredCredentialStore<'a> {
    fn get(
        &self,
        reference: &CredentialRef,
    ) -> Result<SecretString, ssh_manager_core::profiles::CredentialError> {
        match self.ephemeral.get(reference) {
            Ok(secret) => Ok(secret),
            Err(_) => self.real.get(reference),
        }
    }

    fn set(
        &self,
        reference: &CredentialRef,
        secret: SecretString,
    ) -> Result<(), ssh_manager_core::profiles::CredentialError> {
        self.ephemeral.set(reference, secret)
    }

    fn delete(
        &self,
        reference: &CredentialRef,
    ) -> Result<(), ssh_manager_core::profiles::CredentialError> {
        self.ephemeral.delete(reference)
    }
}

fn resolve_secret(
    provided: Option<String>,
    existing: Option<&AuthMethod>,
    real_store: &(dyn CredentialStore + Send + Sync),
    keychain: KeychainAvailability,
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
    real_store
        .get(existing_ref)
        .map_err(|err| keychain_aware_credential_error(err, keychain))
}

#[cfg(test)]
mod tests {
    /// Spec 0071: Diese Tests prüfen den Verbindungstest, nicht die
    /// Schlüsselbund-Verfügbarkeit — der In-Memory-Store ist per Definition
    /// verfügbar.
    const AVAILABLE: KeychainAvailability = KeychainAvailability::Available;

    use ssh_manager_core::profiles::PostIngestPolicy;
    use ssh_manager_core::ssh::mock::MockKeyFileReader;
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
                "execute in StubSshTransport nicht erwartet".to_string(),
            ))
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn InteractiveShell>, SshError> {
            Err(SshError::ChannelError(
                "open_shell in StubSshTransport nicht erwartet".to_string(),
            ))
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    enum MockOutcome {
        Success,
        UnknownHostKey,
        MismatchHostKey,
        AuthenticationFailed,
        NetworkError,
        Timeout,
    }

    struct MockConnector(MockOutcome);
    #[async_trait]
    impl Connector for MockConnector {
        async fn connect(
            &self,
            _target: &ConnectionTarget,
            _credentials: &(dyn CredentialStore + Send + Sync),
            _key_files: &(dyn KeyFileReader + Send + Sync),
            _host_keys: Arc<dyn HostKeyStore>,
        ) -> Result<ConnectOutcome, SshError> {
            match &self.0 {
                MockOutcome::Success => Ok(ConnectOutcome::Connected(Box::new(StubSshTransport))),
                MockOutcome::UnknownHostKey => Ok(ConnectOutcome::PendingHostKeyConfirmation {
                    host: "example.invalid".to_string(),
                    port: 22,
                    raw_key: b"raw-key".to_vec(),
                    decision: HostKeyDecision::Unknown {
                        fingerprint: "SHA256:unknown".to_string(),
                    },
                }),
                MockOutcome::MismatchHostKey => Ok(ConnectOutcome::PendingHostKeyConfirmation {
                    host: "example.invalid".to_string(),
                    port: 22,
                    raw_key: b"raw-key".to_vec(),
                    decision: HostKeyDecision::Mismatch {
                        expected_fingerprint: "SHA256:expected".to_string(),
                        actual_fingerprint: "SHA256:actual".to_string(),
                    },
                }),
                MockOutcome::AuthenticationFailed => Err(SshError::AuthenticationFailed),
                MockOutcome::NetworkError => {
                    Err(SshError::ConnectionFailed("connection refused".to_string()))
                }
                MockOutcome::Timeout => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    Ok(ConnectOutcome::Connected(Box::new(StubSshTransport)))
                }
            }
        }
    }

    fn password_input() -> ServerInput {
        ServerInput {
            name: "test-srv".to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethodInput::Password {
                value: Some("secret".to_string()),
            },
            jump_host: None,
            sudo_password: None,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
        }
    }

    #[tokio::test]
    async fn test_success() {
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(MockOutcome::Success),
            password_input(),
            None,
            Duration::from_millis(200),
        )
        .await
        .unwrap();

        assert!(matches!(result, TestConnectionResult::Success));
    }

    #[tokio::test]
    async fn test_host_key_unknown() {
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(MockOutcome::UnknownHostKey),
            password_input(),
            None,
            Duration::from_millis(200),
        )
        .await
        .unwrap();

        assert!(matches!(
            result,
            TestConnectionResult::HostKeyUnknown { .. }
        ));
    }

    #[tokio::test]
    async fn test_host_key_mismatch() {
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(MockOutcome::MismatchHostKey),
            password_input(),
            None,
            Duration::from_millis(200),
        )
        .await
        .unwrap();

        assert!(matches!(
            result,
            TestConnectionResult::HostKeyMismatch { .. }
        ));
    }

    #[tokio::test]
    async fn test_authentication_failed() {
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(MockOutcome::AuthenticationFailed),
            password_input(),
            None,
            Duration::from_millis(200),
        )
        .await
        .unwrap();

        assert!(matches!(result, TestConnectionResult::AuthenticationFailed));
    }

    #[tokio::test]
    async fn test_network_error() {
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(MockOutcome::NetworkError),
            password_input(),
            None,
            Duration::from_millis(200),
        )
        .await
        .unwrap();

        assert!(matches!(result, TestConnectionResult::NetworkError { .. }));
    }

    #[tokio::test]
    async fn test_timeout() {
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &MockConnector(MockOutcome::Timeout),
            password_input(),
            None,
            Duration::from_millis(50),
        )
        .await
        .unwrap();

        assert!(matches!(result, TestConnectionResult::Timeout));
    }

    #[tokio::test]
    async fn test_empty_secret_without_existing_server_is_a_hard_error_not_a_result() {
        let profile_store = InMemoryProfileStore::new();
        let credential_store = InMemoryCredentialStore::new();

        let input = ServerInput {
            auth: AuthMethodInput::Password { value: None },
            ..password_input()
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &credential_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
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
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
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
            &MockKeyFileReader::new(),
            AVAILABLE,
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

    /// T8: Test-Connection über Jump-Host löst Jump-Host-Credentials aus dem realen Store auf
    /// und Ziel-Hop-Credentials aus dem Ephemeral-Store.
    #[tokio::test]
    async fn test_t8_jump_host_credentials_resolved_from_real_store() {
        use chrono::Utc;
        use ssh_manager_core::profiles::{CredentialRef, Server};

        struct InspectingConnector;
        #[async_trait]
        impl Connector for InspectingConnector {
            async fn connect(
                &self,
                target: &ConnectionTarget,
                credentials: &(dyn CredentialStore + Send + Sync),
                _key_files: &(dyn KeyFileReader + Send + Sync),
                _host_keys: Arc<dyn HostKeyStore>,
            ) -> Result<ConnectOutcome, SshError> {
                assert_eq!(target.hops.len(), 2);

                // Jump host credentials (Hop 0) in real store
                let AuthMethod::Password {
                    credential_ref: jump_ref,
                } = &target.hops[0].auth
                else {
                    panic!("expected password auth for jump host");
                };
                let jump_secret = credentials
                    .get(jump_ref)
                    .expect("jump host secret must resolve");
                assert_eq!(
                    secrecy::ExposeSecret::expose_secret(&jump_secret),
                    "jump-stored-secret"
                );

                // Target hop credentials (Hop 1) in ephemeral store
                let AuthMethod::Password {
                    credential_ref: target_ref,
                } = &target.hops[1].auth
                else {
                    panic!("expected password auth for target host");
                };
                let target_secret = credentials
                    .get(target_ref)
                    .expect("target secret must resolve");
                assert_eq!(
                    secrecy::ExposeSecret::expose_secret(&target_secret),
                    "target-form-secret"
                );

                Ok(ConnectOutcome::Connected(Box::new(StubSshTransport)))
            }
        }

        let jump_id = ServerId::new();
        let jump_pwd_ref = CredentialRef::new("server:jump:password");
        let now = Utc::now();
        let jump_server = Server {
            id: jump_id,
            name: "jump".to_string(),
            host: "jump.invalid".to_string(),
            port: 22,
            username: "jumpuser".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethod::Password {
                credential_ref: jump_pwd_ref.clone(),
            },
            notes: String::new(),
            jump_host: None,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            created_at: now,
            updated_at: now,
        };

        let profile_store = InMemoryProfileStore::new().with_server(jump_server);
        let real_credential_store =
            InMemoryCredentialStore::new().with_secret(&jump_pwd_ref, "jump-stored-secret");

        let input = ServerInput {
            name: "target-srv".to_string(),
            host: "target.invalid".to_string(),
            port: 22,
            username: "targetuser".to_string(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethodInput::Password {
                value: Some("target-form-secret".to_string()),
            },
            jump_host: Some(jump_id),
            sudo_password: None,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &real_credential_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &InspectingConnector,
            input,
            None,
            Duration::from_millis(200),
        )
        .await
        .unwrap();

        assert!(matches!(result, TestConnectionResult::Success));
    }
}
