//! `test_connection` (Spec 0008, Abschnitt 7) — prüft Erreichbarkeit/
//! Zugangsdaten eines (ggf. noch nicht gespeicherten) Servers, ohne
//! irgendetwas zu persistieren.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use secrecy::SecretString;

use ssh_manager_core::profiles::{
    trim_credential_value, AuthMethod, CredentialRef, CredentialStore, ProfileStore,
};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::{
    resolve_connection_target, ConnectionTarget, Hop, HostKeyDecision, HostKeyStore, KeyFileReader,
    SshError,
};
use ssh_transport::ConnectOutcome;

use crate::dto::{AuthMethodInput, ServerInput, TestConnectionResult};
use crate::ephemeral_credentials::EphemeralCredentialStore;
use credentials_keyring::KeychainAvailability;

use crate::error::{
    keychain_aware_credential_error, keychain_aware_ssh_error_code, CommandError, CommandResult,
};

/// Spec 0084, §4 (Schnitt `test_connection` → `commands::SSH_CONNECT_TIMEOUT`):
/// hierher verschoben, weil `test_connection` (Tauri-frei, zieht nach
/// `app-logic`) diese Konstante braucht, `commands::connect` (Tauri-gebunden,
/// bleibt in `app-shell`) sie aber ebenfalls nutzt — die umgekehrte
/// Abhängigkeitsrichtung wäre nach dem Umzug nicht mehr erfüllbar.
///
/// Spec 0069, Teil A3: die bestehende 10-Sekunden-Grenze (ursprünglich
/// `TEST_CONNECTION_TIMEOUT` allein hier) — **keine zweite Konstante**.
/// Umschließt in `connect_session` (`commands::connect`) jeden einzelnen
/// Aufruf von `ssh_transport::connect` (über
/// `ssh_transport::connect_with_timeout`), NIE das Warten auf eine
/// Host-Key-Entscheidung (das bleibt bei `crate::orchestration::
/// PENDING_ACTION_CONFIRM_TIMEOUT`, Spec 0068 Teil 5b) — s.
/// `ssh_transport::connect_with_timeout`s Doc-Kommentar zur
/// Sicherheits-Invariante ("liefert immer einen Fehler, nie `Connected`,
/// nie `trust()`").
pub const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

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
        // Spec 0098, A4: Die Variante des Ergebnisses bleibt `NetworkError`
        // (§5: die Form von `TestConnectionResult` ändert sich nicht, es
        // kommt nur ein Code-Wert hinzu) — der **Code** sagt jetzt aber den
        // Schlüsselbund als Ursache, und das Frontend zeigt dafür weder
        // „Netzwerkfehler" noch „Zugangsdaten konnten nicht aufgelöst
        // werden".
        //
        // `message` kommt unverändert aus `SshError`s `Display`; für
        // `CredentialStoreFailed` ist das per Konstruktion frei von der
        // Nutzlast der Bibliothek (A5).
        Err(other) => TestConnectionResult::NetworkError {
            message: other.to_string(),
            code: Some(keychain_aware_ssh_error_code(&other, keychain)),
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
            let secret = resolve_secret(
                value,
                existing,
                real_credential_store,
                keychain,
                "SERVER_PASSWORD_REQUIRED",
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
                "SERVER_PRIVATE_KEY_REQUIRED",
                |a| match a {
                    AuthMethod::PrivateKey { credential_ref, .. } => Some(credential_ref),
                    _ => None,
                },
            )?;
            let key_ref = CredentialRef::new("test:private_key");
            ephemeral.insert(&key_ref, key_secret);

            // Spec 0073, §9 (K1, BL-0243): dieselbe Trim-Semantik und
            // dieselbe „leer = das Gespeicherte nehmen"-Regel wie beim
            // Speichern (`server_credentials::resolve_auth_method`).
            // Vorher trat der Verbindungstest mit der Passphrase an, wie
            // sie im Feld stand — derselbe Paste konnte den Test scheitern
            // lassen und danach trotzdem richtig gespeichert werden.
            let passphrase_ref = match passphrase.map(|p| trim_credential_value(&p)) {
                Some(p) if !p.is_empty() => {
                    let r = CredentialRef::new("test:passphrase");
                    ephemeral.insert(&r, SecretString::from(p));
                    Some(r)
                }
                _ => match existing {
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
                "SERVER_CERTIFICATE_REQUIRED",
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
                "SERVER_CERTIFICATE_KEY_REQUIRED",
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
            // Spec 0073, §9 (K1, BL-0243): wie im `PrivateKey`-Zweig über
            // den geteilten `trim_credential_value` statt über
            // `str::trim` — und der getrimmte Wert ist auch der, der in
            // den Ephemeral-Store geht, nicht der rohe.
            let passphrase_ref = match passphrase.map(|p| trim_credential_value(&p)) {
                Some(p) if !p.is_empty() => {
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
            // **Nur „kein Eintrag" führt zum nächsten Store** (Spec 0098,
            // spec-reviewer Runde 1): Ein `Backend`-Fehler ist die Störung
            // eines Stores, keine Aussage über den Inhalt. Ihn weiterzugeben
            // hieße, aus einer Störung ein „nicht vorhanden" zu machen — die
            // Rückrichtung von T7, und damit genau die Verwechslung, die
            // Spec 0071 A14/I4 verbietet. Der `EphemeralCredentialStore`
            // liefert heute nur `NotFound`, der Zweig ändert also nichts am
            // Verhalten; er hält den Weg geschlossen.
            Err(ssh_manager_core::profiles::CredentialError::NotFound(_)) => {
                self.real.get(reference)
            }
            Err(backend) => Err(backend),
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

/// Das Secret, mit dem der Verbindungstest antritt — aus dem Formularfeld
/// oder, wenn das leer ist, aus dem gespeicherten Credential des bestehenden
/// Servers.
///
/// Spec 0073, §9 (Q-BL-0149-02): dieselbe Form wie
/// `server_credentials::write_or_reuse_secret` beim Speichern. Der
/// eingegebene Wert läuft über den geteilten [`trim_credential_value`], und
/// **danach** entscheidet dieselbe Leer-Regel: nicht leer → dieser Wert;
/// leer oder nicht angegeben → das gespeicherte Credential; gibt es keins,
/// der Fehler. Vorher nahm dieser Weg jeden `Some`-Wert, auch `""` — das
/// Formular schickt bei Neuanlage für ein leeres Pflichtfeld aber genau
/// `""` (`ServerForm.tsx`, `orNullIfUpdate` greift nur beim Bearbeiten).
/// „Neuer Server, Passwortfeld leer, Verbindung testen" endete damit auf
/// einem Server mit `PermitEmptyPasswords yes` im Erfolg für ein Profil,
/// das sich anschließend nicht speichern ließ.
///
/// Reihenfolge „erst trimmen, dann Leer-Prüfung" wie in Spec 0049, Fund 1:
/// Andernfalls bestünde ein reiner Leerraum-Paste die Leer-Prüfung und
/// würde zu einem Anmeldeversuch mit Leerraum als Secret.
fn resolve_secret(
    provided: Option<String>,
    existing: Option<&AuthMethod>,
    real_store: &(dyn CredentialStore + Send + Sync),
    keychain: KeychainAvailability,
    code: &'static str,
    extract_ref: impl Fn(&AuthMethod) -> Option<&CredentialRef>,
) -> CommandResult<SecretString> {
    // Bewusst dieselbe Gestalt wie `write_or_reuse_secret`: derselbe Trim,
    // dieselbe Bedingung, dieselbe Reihenfolge der Zweige. Weicht eine der
    // beiden Stellen künftig ab, fällt es beim Vergleich auf.
    //
    // Gleiche Regel, nicht gleiches Ergebnis in jedem Fall: Beim Speichern
    // genügt „der Slot existierte vorher", hier wird das Secret zusätzlich
    // gelesen — ist der Schlüsselbund-Eintrag von außen verschwunden, ist
    // Speichern `Ok` und der Test `Err` (ADR 0067 §1).
    match provided.map(|value| trim_credential_value(&value)) {
        Some(value) if !value.is_empty() => Ok(SecretString::from(value)),
        _ => {
            // Spec 0073, §9 (Q-BL-0149-03): derselbe Code wie beim Speichern
            // (`write_or_reuse_secret`), damit beide Knöpfe bei derselben
            // Eingabe denselben übersetzten Satz zeigen. Der Text (nur
            // Rückfall ohne Übersetzung) behauptet nicht „kein Server
            // gefunden": Der Zweig gilt auch für einen bestehenden Server
            // mit anderer Anmeldeart, der für diese Art keinen Slot hat.
            let existing_ref = existing.and_then(extract_ref).ok_or_else(|| {
                CommandError::with_code(
                    "Secret erforderlich (Feld leer, kein hinterlegtes Credential dieser Anmeldeart)",
                    code,
                )
            })?;
            real_store
                .get(existing_ref)
                .map_err(|err| keychain_aware_credential_error(err, keychain))
        }
    }
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

    // --- Spec 0098, Ebene V1 (A4) ------------------------------------------
    //
    // §7: Ein Fehler ab dem zweiten Hop ist gegen den Testserver nicht
    // zuverlässig erreichbar (`resolve_auth` läuft je Hop erst nach TCP,
    // Handshake und Host-Key-Prüfung). Dieser `Connector` schließt die Lücke:
    // Er ruft für **jeden** Hop die echte Auflösung `resolve_auth` mit dem
    // Test-Store auf — dieselbe Funktion, die `ssh-transport` aufruft — und
    // benennt den Hop über `SshError::named_for_hop`, also über **dieselbe**
    // Funktion, die `ssh_transport::auth::name_hop` aufruft. Geprüft wird
    // damit die Kette `CredentialStore` → `resolve_auth` → `SshError` →
    // `TestConnectionResult`, ohne eine echte Verbindung.
    //
    // Die Hop-Benennung war hier zunächst **nachgebaut**. Ein Nachbau prüft
    // sich selbst: Hörte `name_hop` auf, die neue Variante zu benennen,
    // blieben T5/T12/T13 grün (spec-reviewer Runde 1). Deshalb liegt die
    // Benennung jetzt in `core` und beide Seiten rufen sie auf.
    struct ResolvingConnector;

    #[async_trait]
    impl Connector for ResolvingConnector {
        async fn connect(
            &self,
            target: &ConnectionTarget,
            credentials: &(dyn CredentialStore + Send + Sync),
            key_files: &(dyn KeyFileReader + Send + Sync),
            _host_keys: Arc<dyn HostKeyStore>,
        ) -> Result<ConnectOutcome, SshError> {
            for hop in &target.hops {
                ssh_manager_core::ssh::resolve_auth(&hop.auth, credentials, key_files)
                    // Genau der Aufruf, den `ssh_transport::auth::name_hop`
                    // macht — der Prüfpunkt aus Spec 0076, A-8 gilt auch hier.
                    .map_err(|err| err.named_for_hop(&hop.username, &hop.host, hop.port))?;
            }
            Ok(ConnectOutcome::Connected(Box::new(StubSshTransport)))
        }
    }

    /// Ein gespeicherter Server mit Passwort-Anmeldung, optional hinter einem
    /// weiteren Jump-Host.
    fn stored_server(
        id: ServerId,
        name: &str,
        password_ref: &ssh_manager_core::profiles::CredentialRef,
        jump_host: Option<ServerId>,
    ) -> ssh_manager_core::profiles::Server {
        let now = chrono::Utc::now();
        ssh_manager_core::profiles::Server {
            id,
            name: name.to_string(),
            host: format!("{name}.invalid"),
            port: 22,
            username: format!("{name}user"),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethod::Password {
                credential_ref: password_ref.clone(),
            },
            notes: String::new(),
            jump_host,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Spec 0098, T5 (A4, Jump-Host, Ebene V1) — die Akzeptanz aus BL-0206.
    /// Das Passwort-Lesen des Jump-Hosts scheitert mit einem Backend-Fehler;
    /// das Ergebnis trägt `KEYCHAIN_ACCESS_FAILED` und keine Nutzlast.
    ///
    /// Scheitert am Stand vor dieser Spec: dort `SSH_CREDENTIAL_RESOLUTION_
    /// FAILED` mit dem Marker in `message` („✗ Netzwerkfehler: Passwort:
    /// …").
    #[tokio::test]
    async fn test_spec_0098_t5_a_failing_jump_host_secret_reports_the_keychain() {
        let jump_id = ServerId::new();
        let jump_ref = ssh_manager_core::profiles::CredentialRef::new("server:jump:password");
        let profile_store = InMemoryProfileStore::new()
            .with_server(stored_server(jump_id, "jump", &jump_ref, None));
        let real_store = InMemoryCredentialStore::new()
            .with_secret(&jump_ref, "jump-stored-secret")
            .with_failing_get_for_slot("password")
            .with_backend_payload("LIBTEXT-0098 Geheim-0098");

        let input = ServerInput {
            jump_host: Some(jump_id),
            ..password_input()
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &real_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &ResolvingConnector,
            input,
            None,
            Duration::from_millis(200),
        )
        .await
        .expect("der Verbindungstest liefert ein Ergebnis, keinen CommandError");

        let TestConnectionResult::NetworkError { message, code } = &result else {
            panic!("erwartet NetworkError, bekam {result:?}");
        };
        assert_eq!(*code, Some(crate::error::KEYCHAIN_ACCESS_FAILED));
        assert!(
            !message.contains("LIBTEXT-0098") && !message.contains("Geheim-0098"),
            "die Nutzlast der Bibliothek darf nicht im DTO stehen: {message}"
        );
        // Spec 0076, A-8: Welcher Hop es war, bleibt sichtbar.
        assert!(
            message.contains("jumpuser@jump.invalid:22"),
            "die Hop-Angabe gehört in die Meldung: {message}"
        );
    }

    /// Spec 0098, T5 (A2-Hälfte): derselbe Fall bei einem Schlüsselbund, der
    /// schon beim Start fehlte → `KEYCHAIN_UNAVAILABLE`, weiterhin ohne
    /// Nutzlast.
    #[tokio::test]
    async fn test_spec_0098_t5_a_failing_jump_host_secret_reports_unavailable_at_startup() {
        let jump_id = ServerId::new();
        let jump_ref = ssh_manager_core::profiles::CredentialRef::new("server:jump:password");
        let profile_store = InMemoryProfileStore::new()
            .with_server(stored_server(jump_id, "jump", &jump_ref, None));
        let real_store = InMemoryCredentialStore::new()
            .with_secret(&jump_ref, "jump-stored-secret")
            .with_failing_get_for_slot("password")
            .with_backend_payload("LIBTEXT-0098 Geheim-0098");

        let input = ServerInput {
            jump_host: Some(jump_id),
            ..password_input()
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &real_store,
            &MockKeyFileReader::new(),
            KeychainAvailability::Unavailable(
                credentials_keyring::KeychainUnavailableReason::NoSecretServiceProvider,
            ),
            Arc::new(NoOpHostKeyStore),
            &ResolvingConnector,
            input,
            None,
            Duration::from_millis(200),
        )
        .await
        .expect("der Verbindungstest liefert ein Ergebnis, keinen CommandError");

        let TestConnectionResult::NetworkError { message, code } = &result else {
            panic!("erwartet NetworkError, bekam {result:?}");
        };
        assert_eq!(*code, Some(crate::error::KEYCHAIN_UNAVAILABLE));
        assert!(
            !message.contains("Geheim-0098"),
            "die Nutzlast bleibt auch hier draußen: {message}"
        );
    }

    /// Spec 0098, T7 (A4, `NotFound`): Fehlt der Eintrag des Jump-Hosts
    /// ganz, bleibt es bei `SSH_CREDENTIAL_RESOLUTION_FAILED` — „kein
    /// Eintrag" ist keine Störung des Schlüsselbunds (Spec 0071 A14/I4).
    ///
    /// Die Gegenprobe zu T5: Ohne sie könnte die Umstellung alle
    /// Credential-Fehler als Schlüsselbund-Fehler melden und T5 wäre
    /// trotzdem grün.
    #[tokio::test]
    async fn test_spec_0098_t7_a_missing_jump_host_entry_is_not_a_keychain_failure() {
        let jump_id = ServerId::new();
        let jump_ref = ssh_manager_core::profiles::CredentialRef::new("server:jump:password");
        let profile_store = InMemoryProfileStore::new()
            .with_server(stored_server(jump_id, "jump", &jump_ref, None));
        // Kein `with_secret` und kein Fehler-Schalter: der Store antwortet,
        // es gibt bloß nichts.
        let real_store = InMemoryCredentialStore::new();

        let input = ServerInput {
            jump_host: Some(jump_id),
            ..password_input()
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &real_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &ResolvingConnector,
            input,
            None,
            Duration::from_millis(200),
        )
        .await
        .expect("der Verbindungstest liefert ein Ergebnis, keinen CommandError");

        let TestConnectionResult::NetworkError { code, .. } = &result else {
            panic!("erwartet NetworkError, bekam {result:?}");
        };
        assert_eq!(*code, Some("SSH_CREDENTIAL_RESOLUTION_FAILED"));
    }

    /// Spec 0098, T12 (adversarial, Ebene V1): Kette mit drei Hops, der
    /// Fehler sitzt am **mittleren**. Der Code folgt A4 und die Meldung nennt
    /// den mittleren Hop — nicht den ersten und nicht den letzten.
    ///
    /// Die Hops der Kette heißen unterschiedlich benannte Slots, damit der
    /// Store gezielt nur einen davon verweigern kann; ein Test mit drei
    /// gleichen Slots könnte nicht zeigen, **welcher** Hop gescheitert ist.
    #[tokio::test]
    async fn test_spec_0098_t12_a_three_hop_chain_names_the_middle_hop() {
        let first_id = ServerId::new();
        let middle_id = ServerId::new();
        let first_ref = ssh_manager_core::profiles::CredentialRef::new("server:first:password");
        let middle_ref = ssh_manager_core::profiles::CredentialRef::new("server:middle:middlepwd");

        // `middle` hängt hinter `first`: resolve_connection_target baut
        // daraus die Kette first → middle, der Formular-Hop kommt dahinter.
        let profile_store = InMemoryProfileStore::new()
            .with_server(stored_server(first_id, "first", &first_ref, None))
            .with_server(stored_server(
                middle_id,
                "middle",
                &middle_ref,
                Some(first_id),
            ));
        let real_store = InMemoryCredentialStore::new()
            .with_secret(&first_ref, "first-secret")
            .with_secret(&middle_ref, "middle-secret")
            .with_failing_get_for_slot("middlepwd")
            .with_backend_payload("LIBTEXT-0098 Geheim-0098");

        let input = ServerInput {
            jump_host: Some(middle_id),
            ..password_input()
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &real_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &ResolvingConnector,
            input,
            None,
            Duration::from_millis(200),
        )
        .await
        .expect("der Verbindungstest liefert ein Ergebnis, keinen CommandError");

        let TestConnectionResult::NetworkError { message, code } = &result else {
            panic!("erwartet NetworkError, bekam {result:?}");
        };
        assert_eq!(*code, Some(crate::error::KEYCHAIN_ACCESS_FAILED));
        assert!(
            message.contains("middleuser@middle.invalid:22"),
            "der mittlere Hop muss benannt sein: {message}"
        );
        assert!(
            !message.contains("firstuser@") && !message.contains("deploy@"),
            "und zwar nur er: {message}"
        );
        assert!(
            !message.contains("Geheim-0098"),
            "die Nutzlast bleibt draußen: {message}"
        );
    }

    /// Spec 0098, T13 (A4 für jede Secret-Art): Nicht nur „Passwort" — die
    /// **Passphrase** eines Private Keys auf dem Jump-Host scheitert. A4
    /// gilt für alle Arten aus `resolve_auth`; ohne diesen Test wäre nur der
    /// Passwort-Zweig belegt.
    #[tokio::test]
    async fn test_spec_0098_t13_a_failing_jump_host_passphrase_reports_the_keychain() {
        let jump_id = ServerId::new();
        let key_ref = ssh_manager_core::profiles::CredentialRef::new("server:jump:private_key");
        let passphrase_ref =
            ssh_manager_core::profiles::CredentialRef::new("server:jump:passphrase");

        let now = chrono::Utc::now();
        let jump_server = ssh_manager_core::profiles::Server {
            auth: AuthMethod::PrivateKey {
                credential_ref: key_ref.clone(),
                passphrase_ref: Some(passphrase_ref.clone()),
            },
            created_at: now,
            updated_at: now,
            ..stored_server(jump_id, "jump", &key_ref, None)
        };
        let profile_store = InMemoryProfileStore::new().with_server(jump_server);
        let real_store = InMemoryCredentialStore::new()
            .with_secret(&key_ref, "key-material")
            .with_secret(&passphrase_ref, "phrase")
            // Nur die Passphrase scheitert — der Key selbst wird gelesen.
            // Das belegt, dass die Secret-Art mitgeführt wird und nicht
            // pauschal der erste Lesefehler gemeldet wird.
            .with_failing_get_for_slot("passphrase")
            .with_backend_payload("LIBTEXT-0098 Geheim-0098");

        let input = ServerInput {
            jump_host: Some(jump_id),
            ..password_input()
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &real_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &ResolvingConnector,
            input,
            None,
            Duration::from_millis(200),
        )
        .await
        .expect("der Verbindungstest liefert ein Ergebnis, keinen CommandError");

        let TestConnectionResult::NetworkError { message, code } = &result else {
            panic!("erwartet NetworkError, bekam {result:?}");
        };
        assert_eq!(*code, Some(crate::error::KEYCHAIN_ACCESS_FAILED));
        assert!(
            message.contains("Passphrase"),
            "die Art des Secrets gehört in die Meldung: {message}"
        );
        assert!(
            !message.contains("Geheim-0098"),
            "die Nutzlast bleibt draußen: {message}"
        );
    }

    /// Spec 0098, T13 / spec-reviewer Runde 1 (adversarialer Fall 6): Der
    /// **Zertifikats**-Zweig von `resolve_auth` liest zwei Secrets. Hier
    /// gelingt das erste (das Zertifikat) und erst das zweite (der Key)
    /// scheitert.
    ///
    /// Der Fall prüft zweierlei, was mit einem einzigen `get` nicht zu
    /// trennen wäre: dass auch der **zweite** Lesefehler eines Zweigs den
    /// Schlüsselbund-Code trägt (nicht nur der erste), und dass die
    /// mitgeführte Secret-Art die des gescheiterten Aufrufs ist („Key",
    /// nicht „Zertifikat").
    #[tokio::test]
    async fn test_spec_0098_t13_a_failing_certificate_key_names_the_key_not_the_certificate() {
        let jump_id = ServerId::new();
        let cert_ref = ssh_manager_core::profiles::CredentialRef::new("server:jump:certificate");
        let key_ref = ssh_manager_core::profiles::CredentialRef::new("server:jump:certkey");

        let now = chrono::Utc::now();
        let jump_server = ssh_manager_core::profiles::Server {
            auth: AuthMethod::Certificate {
                cert_ref: cert_ref.clone(),
                key_ref: key_ref.clone(),
            },
            created_at: now,
            updated_at: now,
            ..stored_server(jump_id, "jump", &cert_ref, None)
        };
        let profile_store = InMemoryProfileStore::new().with_server(jump_server);
        let real_store = InMemoryCredentialStore::new()
            .with_secret(&cert_ref, "cert-material")
            .with_secret(&key_ref, "key-material")
            // Nur der Key scheitert; das Zertifikat davor wird gelesen.
            .with_failing_get_for_slot("certkey")
            .with_backend_payload("LIBTEXT-0098 Geheim-0098");

        let input = ServerInput {
            jump_host: Some(jump_id),
            ..password_input()
        };

        let result = test_connection_with_timeout(
            &profile_store,
            &real_store,
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &ResolvingConnector,
            input,
            None,
            Duration::from_millis(200),
        )
        .await
        .expect("der Verbindungstest liefert ein Ergebnis, keinen CommandError");

        let TestConnectionResult::NetworkError { message, code } = &result else {
            panic!("erwartet NetworkError, bekam {result:?}");
        };
        assert_eq!(*code, Some(crate::error::KEYCHAIN_ACCESS_FAILED));
        assert!(
            message.contains("Key"),
            "die Art des gescheiterten Secrets gehört in die Meldung: {message}"
        );
        assert!(
            !message.contains("Zertifikat"),
            "und nicht die des Secrets, das gelesen werden konnte: {message}"
        );
        assert!(
            !message.contains("Geheim-0098"),
            "die Nutzlast bleibt draußen: {message}"
        );
    }

    /// Spec 0098, T6 (A4, Ziel-Hop): **Der Ziel-Hop liest sein Secret nicht
    /// in der Kette, sondern davor.** Das ist ein Befund zur Spec, die für
    /// den Ziel-Hop „Ebene V1" vorsieht:
    ///
    /// - Ist das Formularfeld gefüllt, liegt das Secret im
    ///   `EphemeralCredentialStore` — der scheitert nie, ein
    ///   Schlüsselbund-Fehler kann dort gar nicht entstehen.
    /// - Ist es leer, liest `resolve_final_hop_auth` das gespeicherte Secret
    ///   **vor** dem Verbindungsversuch (`resolve_secret`). Der Fehler kommt
    ///   dann als `CommandError`, nicht als `TestConnectionResult` — das ist
    ///   der Weg, den T2 prüft.
    ///
    /// Dieser Test hält beide Hälften zusammen fest, damit der Befund nicht
    /// als Lücke gelesen wird: Mit **demselben** auflösenden Connector wie
    /// T5/T12/T13 kommt der Fehler des Ziel-Hops vorab und mit dem richtigen
    /// Code — und mit gefülltem Feld läuft die Kette durch, obwohl der reale
    /// Store für denselben Slot scheitert.
    #[tokio::test]
    async fn test_spec_0098_t6_the_target_hop_secret_is_resolved_before_the_chain() {
        let existing_id = ServerId::new();
        let password_ref = ssh_manager_core::profiles::CredentialRef::new("server:target:password");
        let failing_real_store = || {
            InMemoryCredentialStore::new()
                .with_secret(&password_ref, "stored-secret")
                .with_failing_get_for_slot("password")
                .with_backend_payload("LIBTEXT-0098 Geheim-0098")
        };
        let profile_store = || {
            InMemoryProfileStore::new().with_server(stored_server(
                existing_id,
                "target",
                &password_ref,
                None,
            ))
        };

        // Hälfte 1 — leeres Feld: das gespeicherte Secret wird vorab gelesen,
        // und genau das scheitert.
        let err = test_connection_with_timeout(
            &profile_store(),
            &failing_real_store(),
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &ResolvingConnector,
            ServerInput {
                auth: AuthMethodInput::Password { value: None },
                ..password_input()
            },
            Some(existing_id),
            Duration::from_millis(200),
        )
        .await
        .expect_err("ein nicht lesbares Ziel-Secret darf keinen Verbindungstest auslösen");

        assert_eq!(err.code, Some(crate::error::KEYCHAIN_ACCESS_FAILED));
        assert!(
            !err.message.contains("Geheim-0098"),
            "die Nutzlast bleibt draußen: {}",
            err.message
        );

        // Hälfte 2 — gefülltes Feld: derselbe scheiternde Store, die Kette
        // läuft trotzdem durch. Belegt, dass der Ziel-Hop in der Kette aus
        // dem Ephemeral-Store liest und dort kein Schlüsselbund im Spiel ist.
        let result = test_connection_with_timeout(
            &profile_store(),
            &failing_real_store(),
            &MockKeyFileReader::new(),
            AVAILABLE,
            Arc::new(NoOpHostKeyStore),
            &ResolvingConnector,
            password_input(),
            Some(existing_id),
            Duration::from_millis(200),
        )
        .await
        .expect("der Verbindungstest liefert ein Ergebnis, keinen CommandError");

        assert!(
            matches!(result, TestConnectionResult::Success),
            "mit gefülltem Feld berührt der Ziel-Hop den Schlüsselbund nicht: {result:?}"
        );
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

    /// Spec 0098, T2 (A1, Lesen): Dasselbe wie der Test darüber, nur
    /// scheitert der Schlüsselbund beim Lesen des gespeicherten Passworts.
    /// Das ist der lesende Weg aus BL-0205 — ein nach dem Start gesperrter
    /// Schlüsselbund —, und er ging bislang als codeloser Bibliothekstext
    /// ins Server-Formular.
    ///
    /// Scheitert am Stand vor dieser Spec (`code: None`, Marker in
    /// `message`).
    ///
    /// Der Verbindungstest kommt hier gar nicht bis zum `Connector`: Das
    /// Secret des Ziel-Hops wird **vor** dem Verbindungsversuch aufgelöst.
    /// Deshalb ein `CommandError`, kein `TestConnectionResult` — den Weg
    /// über die Verbindungskette prüfen T5/T6/T12.
    #[tokio::test]
    async fn test_spec_0098_t2_failed_read_of_a_stored_secret_reports_the_access_code() {
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
        // Der Wert **ist** hinterlegt — der Store kann es nur nicht sagen.
        // Genau der Fall, der sich von „kein Eintrag" unterscheiden muss.
        let credential_store = InMemoryCredentialStore::new()
            .with_secret(&password_ref, "stored-secret")
            .with_failing_get_for_slot("password")
            .with_backend_payload("LIBTEXT-0098 Geheim-0098");

        let input = ServerInput {
            auth: AuthMethodInput::Password { value: None },
            ..password_input()
        };

        let err = test_connection_with_timeout(
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
        .expect_err("ein nicht lesbares Secret darf keinen Verbindungstest auslösen");

        assert_eq!(err.code, Some(crate::error::KEYCHAIN_ACCESS_FAILED));
        assert!(
            !err.message.contains("LIBTEXT-0098") && !err.message.contains("Geheim-0098"),
            "die Nutzlast der Bibliothek darf das Frontend nicht erreichen: {}",
            err.message
        );
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

    // --- Spec 0073, §9 (K1, BL-0243): Passphrase im Verbindungstest ------
    //
    // „Verbindung testen" und „Speichern" bekommen dieselbe Eingabe aus
    // demselben Formularfeld. Behandeln sie sie verschieden, sagt ein
    // grüner Verbindungstest nichts über den Server aus, der danach
    // gespeichert wird — und ein roter nichts über die Passphrase. Beide
    // Wege laufen deshalb über denselben `trim_credential_value` mit
    // derselben „leer = das Gespeicherte nehmen"-Regel.
    //
    // Geprüft wird je Anmeldeart mit Passphrase (`PrivateKey`,
    // `IdentityFile`), dass die **Ergebnisse** übereinstimmen, nicht dass
    // beide Stellen denselben Funktionsnamen aufrufen.

    #[derive(Clone, Copy, Debug)]
    enum PassphraseAuth {
        PrivateKey,
        IdentityFile,
    }

    const PASSPHRASE_AUTH_KINDS: [PassphraseAuth; 2] =
        [PassphraseAuth::PrivateKey, PassphraseAuth::IdentityFile];

    impl PassphraseAuth {
        fn input(self, passphrase: Option<&str>) -> AuthMethodInput {
            let passphrase = passphrase.map(str::to_string);
            match self {
                PassphraseAuth::PrivateKey => AuthMethodInput::PrivateKey {
                    key_content: Some("-----BEGIN OPENSSH PRIVATE KEY-----".to_string()),
                    passphrase,
                },
                PassphraseAuth::IdentityFile => AuthMethodInput::IdentityFile {
                    path: "/home/deploy/.ssh/id_ed25519".to_string(),
                    passphrase,
                },
            }
        }

        fn existing(self, passphrase_ref: CredentialRef) -> AuthMethod {
            match self {
                PassphraseAuth::PrivateKey => AuthMethod::PrivateKey {
                    credential_ref: CredentialRef::new("server:existing:private_key"),
                    passphrase_ref: Some(passphrase_ref),
                },
                PassphraseAuth::IdentityFile => AuthMethod::IdentityFile {
                    path: "/home/deploy/.ssh/id_ed25519".to_string(),
                    passphrase_ref: Some(passphrase_ref),
                },
            }
        }
    }

    fn passphrase_ref_of(auth: &AuthMethod) -> Option<CredentialRef> {
        match auth {
            AuthMethod::PrivateKey { passphrase_ref, .. }
            | AuthMethod::IdentityFile { passphrase_ref, .. } => passphrase_ref.clone(),
            other => panic!("unerwartete AuthMethod ohne Passphrase-Slot: {other:?}"),
        }
    }

    fn expose(store: &dyn CredentialStore, r: &CredentialRef) -> String {
        use secrecy::ExposeSecret;
        store
            .get(r)
            .expect("Passphrase-Slot muss auflösbar sein")
            .expose_secret()
            .to_string()
    }

    /// Dieselbe Eingabe durch beide Wege. Zurück kommt je Weg die
    /// Passphrase, mit der tatsächlich angemeldet würde — `None`, wenn
    /// kein Passphrase-Slot entstand.
    ///
    /// `stored` ist die bereits hinterlegte Passphrase eines bestehenden
    /// Servers (`None` = neuer Server ohne Vorgeschichte). Die beiden Wege
    /// bekommen getrennte, gleich befüllte Stores, damit der schreibende
    /// Speicher-Weg dem lesenden Test-Weg nicht die Ausgangslage
    /// verändert.
    fn passphrase_both_ways(
        kind: PassphraseAuth,
        pasted: Option<&str>,
        stored: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let existing_ref = CredentialRef::new("server:existing:passphrase");
        let existing_auth = stored.map(|_| kind.existing(existing_ref.clone()));

        let seed = || match stored {
            Some(value) => InMemoryCredentialStore::new().with_secret(&existing_ref, value),
            None => InMemoryCredentialStore::new(),
        };

        // Weg 1: „Verbindung testen" — Secrets landen im Ephemeral-Store.
        let real_store = seed();
        let ephemeral = EphemeralCredentialStore::new();
        let test_auth = resolve_final_hop_auth(
            &ephemeral,
            &real_store,
            AVAILABLE,
            kind.input(pasted),
            existing_auth.as_ref(),
        )
        .expect("Verbindungstest-Auth muss auflösen");
        let via_test =
            passphrase_ref_of(&test_auth).map(|r| expose(&ephemeral as &dyn CredentialStore, &r));

        // Weg 2: „Speichern" — Secrets landen im echten Store.
        let save_store = seed();
        let save_auth = crate::server_credentials::resolve_auth_method(
            &save_store,
            AVAILABLE,
            ServerId::new(),
            kind.input(pasted),
            existing_auth.as_ref(),
        )
        .expect("Speicher-Auth muss auflösen");
        let via_save =
            passphrase_ref_of(&save_auth).map(|r| expose(&save_store as &dyn CredentialStore, &r));

        (via_test, via_save)
    }

    #[test]
    fn test_bl0243_pasted_passphrase_is_trimmed_the_same_way_in_both_paths() {
        for kind in PASSPHRASE_AUTH_KINDS {
            let (via_test, via_save) =
                passphrase_both_ways(kind, Some(" \u{FEFF}geheime-passphrase\u{200B}\r\n"), None);

            assert_eq!(
                via_test, via_save,
                "{kind:?}: Verbindungstest und Speichern müssen dieselbe Passphrase benutzen"
            );
            assert_eq!(
                via_test.as_deref(),
                Some("geheime-passphrase"),
                "{kind:?}: Rand-Leerraum, BOM und Zero-Width-Space gehören weggetrimmt"
            );
        }
    }

    #[test]
    fn test_bl0243_passphrase_of_only_invisible_chars_keeps_the_stored_one_in_both_paths() {
        for kind in PASSPHRASE_AUTH_KINDS {
            let (via_test, via_save) =
                passphrase_both_ways(kind, Some("\u{200B} \u{FEFF}"), Some("gespeicherte-alte"));

            assert_eq!(
                via_test, via_save,
                "{kind:?}: ein Paste aus lauter unsichtbaren Zeichen muss in beiden Wegen \
                 dasselbe heißen"
            );
            assert_eq!(
                via_test.as_deref(),
                Some("gespeicherte-alte"),
                "{kind:?}: leer heißt „das Hinterlegte nehmen\", nicht „mit leer anmelden\""
            );
        }
    }

    // Gegenstück zu I1: Nur der Rand wird bereinigt. Eine Passphrase mit
    // einem unsichtbaren Zeichen **innen** behält es — in beiden Wegen.
    #[test]
    fn test_bl0243_invisible_char_inside_the_passphrase_survives_in_both_paths() {
        for kind in PASSPHRASE_AUTH_KINDS {
            let (via_test, via_save) =
                passphrase_both_ways(kind, Some("  ge\u{200B}heim  "), Some("gespeicherte-alte"));

            assert_eq!(via_test, via_save, "{kind:?}");
            assert_eq!(
                via_test.as_deref(),
                Some("ge\u{200B}heim"),
                "{kind:?}: ein Zeichen innerhalb der Passphrase gehört dort möglicherweise hin"
            );
        }
    }

    // --- Spec 0073, §9 (Q-BL-0149-02): die übrigen Secret-Slots -----------
    //
    // Dieselbe Begründung wie beim Passphrase-Block oben, jetzt für die
    // Pflicht-Secrets selbst: Passwort, Key-Inhalt, Zertifikat und
    // Zertifikats-Key. Zwei Unterschiede waren zu schließen — der Trim und
    // die Bedeutung von „leer". Geprüft wird je Slot, dass beide Wege
    // dasselbe Secret benutzen bzw. beide denselben Ausgang nehmen, nicht
    // dass sie denselben Funktionsnamen aufrufen.

    #[derive(Clone, Copy, Debug)]
    enum SecretSlot {
        Password,
        PrivateKeyContent,
        CertificateContent,
        CertificateKeyContent,
    }

    const SECRET_SLOTS: [SecretSlot; 4] = [
        SecretSlot::Password,
        SecretSlot::PrivateKeyContent,
        SecretSlot::CertificateContent,
        SecretSlot::CertificateKeyContent,
    ];

    /// Füllt das jeweils **andere** Pflichtfeld derselben Anmeldeart, damit
    /// ein Fehler eindeutig vom geprüften Slot kommt und nicht vom Nachbarn.
    const SIBLING_FILLER: &str = "-----BEGIN CERTIFICATE----- nachbar";

    /// Die leeren Eingaben, die das Formular tatsächlich schicken kann:
    /// ein echtes Leerfeld bei Neuanlage (`""`, s. `ServerForm.tsx`s
    /// `orNullIfUpdate`), ein Leerraum-Paste und ein Paste aus lauter
    /// unsichtbaren Zeichen.
    const EMPTY_INPUTS: [&str; 3] = ["", "   \r\n", "\u{200B} \u{FEFF}"];

    impl SecretSlot {
        fn input(self, value: Option<&str>) -> AuthMethodInput {
            let value = value.map(str::to_string);
            let filler = || Some(SIBLING_FILLER.to_string());
            match self {
                SecretSlot::Password => AuthMethodInput::Password { value },
                SecretSlot::PrivateKeyContent => AuthMethodInput::PrivateKey {
                    key_content: value,
                    passphrase: None,
                },
                SecretSlot::CertificateContent => AuthMethodInput::Certificate {
                    cert_content: value,
                    key_content: filler(),
                },
                SecretSlot::CertificateKeyContent => AuthMethodInput::Certificate {
                    cert_content: filler(),
                    key_content: value,
                },
            }
        }

        /// Der Code, den das **Speichern** für ein fehlendes Pflichtfeld dieses
        /// Slots vergibt (`server_credentials::resolve_auth_method`). Hier
        /// bewusst als Literal, nicht aus dem Speicher-Weg abgeleitet — sonst
        /// prüfte der Test nur, dass beide Wege gleich lügen.
        fn required_code(self) -> &'static str {
            match self {
                SecretSlot::Password => "SERVER_PASSWORD_REQUIRED",
                SecretSlot::PrivateKeyContent => "SERVER_PRIVATE_KEY_REQUIRED",
                SecretSlot::CertificateContent => "SERVER_CERTIFICATE_REQUIRED",
                SecretSlot::CertificateKeyContent => "SERVER_CERTIFICATE_KEY_REQUIRED",
            }
        }

        /// Ein Slot einer **anderen** Anmeldeart als `self` — Ausgangslage
        /// „bestehender Server mit anderer Anmeldeart".
        fn other_method(self) -> SecretSlot {
            match self {
                SecretSlot::Password => SecretSlot::PrivateKeyContent,
                _ => SecretSlot::Password,
            }
        }

        /// Der Credential-Slot, aus dem sich das Secret ergibt, mit dem
        /// angemeldet würde — aus der zurückgegebenen `AuthMethod` gelesen,
        /// damit derselbe Helfer für beide Wege taugt (die Refs heißen
        /// verschieden: `test:password` gegen `server:{id}:password`).
        fn secret_ref_of(self, auth: &AuthMethod) -> CredentialRef {
            match (self, auth) {
                (SecretSlot::Password, AuthMethod::Password { credential_ref })
                | (SecretSlot::PrivateKeyContent, AuthMethod::PrivateKey { credential_ref, .. }) => {
                    credential_ref.clone()
                }
                (SecretSlot::CertificateContent, AuthMethod::Certificate { cert_ref, .. }) => {
                    cert_ref.clone()
                }
                (SecretSlot::CertificateKeyContent, AuthMethod::Certificate { key_ref, .. }) => {
                    key_ref.clone()
                }
                (slot, other) => panic!("{slot:?}: unerwartete AuthMethod {other:?}"),
            }
        }
    }

    /// Der vollständige Inhalt eines Stores als sortierte (Ref, Wert)-Liste.
    /// Grundlage der Gegenprobe, dass der Verbindungstest nur liest.
    fn snapshot(store: &InMemoryCredentialStore) -> Vec<(String, String)> {
        use secrecy::ExposeSecret;
        let mut entries: Vec<(String, String)> = store
            .secrets
            .lock()
            .unwrap()
            .iter()
            .map(|(r, secret)| (r.clone(), secret.expose_secret().to_string()))
            .collect();
        entries.sort();
        entries
    }

    /// Was ein Weg meldet, wenn er abbricht: der stabile Code fürs Frontend
    /// und der Rückfalltext.
    #[derive(Debug, PartialEq)]
    struct Refusal {
        code: Option<&'static str>,
        message: String,
    }

    /// Dieselbe Eingabe durch beide Wege. Zurück kommt je Weg das Secret,
    /// mit dem tatsächlich angemeldet würde — oder die Ablehnung, wenn
    /// der Weg abbricht.
    ///
    /// `stored` ist das bereits hinterlegte Secret eines bestehenden Servers
    /// derselben Anmeldeart (`None` = Neuanlage ohne Vorgeschichte).
    fn secret_both_ways(
        slot: SecretSlot,
        pasted: Option<&str>,
        stored: Option<&str>,
    ) -> (Result<String, Refusal>, Result<String, Refusal>) {
        secret_both_ways_from(slot, pasted, &|| stored.map(|v| slot.input(Some(v))))
    }

    /// Wie [`secret_both_ways`], aber der bestehende Server entsteht aus
    /// einer beliebigen Eingabe — auch einer anderen Anmeldeart. Der
    /// Ausgangszustand entsteht durch einen echten Speicher-Vorgang, damit
    /// die Credential-Refs genau die sind, die `update_server` später
    /// wiederverwendet. Beide Wege bekommen getrennte, gleich befüllte
    /// Stores, damit der schreibende Speicher-Weg dem lesenden Test-Weg die
    /// Ausgangslage nicht verändert.
    fn secret_both_ways_from(
        slot: SecretSlot,
        pasted: Option<&str>,
        existing_input: &dyn Fn() -> Option<AuthMethodInput>,
    ) -> (Result<String, Refusal>, Result<String, Refusal>) {
        let server_id = ServerId::new();

        let seed = || {
            let store = InMemoryCredentialStore::new();
            let existing = existing_input().map(|input| {
                crate::server_credentials::resolve_auth_method(
                    &store, AVAILABLE, server_id, input, None,
                )
                .expect("Ausgangszustand des bestehenden Servers muss speicherbar sein")
            });
            (store, existing)
        };
        let refusal = |err: CommandError| Refusal {
            code: err.code,
            message: err.message,
        };

        // Weg 1: „Verbindung testen" — Secrets landen im Ephemeral-Store.
        let (real_store, existing) = seed();
        let store_before = snapshot(&real_store);
        let ephemeral = EphemeralCredentialStore::new();
        let via_test = resolve_final_hop_auth(
            &ephemeral,
            &real_store,
            AVAILABLE,
            slot.input(pasted),
            existing.as_ref(),
        )
        .map(|auth| {
            expose(
                &ephemeral as &dyn CredentialStore,
                &slot.secret_ref_of(&auth),
            )
        })
        .map_err(refusal);

        // Gegenprobe zur Modul-Zusage „ohne irgendetwas zu persistieren":
        // Der echte Store muss nach dem Test-Weg Zeichen für Zeichen so
        // aussehen wie davor. Alle Tests tragen sie mit, auch der
        // Neuanlage-Fall, in dem es kein hinterlegtes Credential zum
        // Vergleichen gibt — verglichen wird der **ganze** Inhalt, nicht
        // eine Liste erwarteter `test:*`-Refs: Ein künftiger Slot, den
        // niemand in eine solche Liste einträgt, fällt so trotzdem auf,
        // und ein Schreiben auf den Nachbar-Slot ebenso.
        assert_eq!(
            snapshot(&real_store),
            store_before,
            "{slot:?}: der Verbindungstest hat den echten Schlüsselbund verändert — \
             er darf ausschließlich lesen"
        );

        // Weg 2: „Speichern" — Secrets landen im echten Store.
        let (save_store, existing) = seed();
        let via_save = crate::server_credentials::resolve_auth_method(
            &save_store,
            AVAILABLE,
            server_id,
            slot.input(pasted),
            existing.as_ref(),
        )
        .map(|auth| {
            expose(
                &save_store as &dyn CredentialStore,
                &slot.secret_ref_of(&auth),
            )
        })
        .map_err(refusal);

        (via_test, via_save)
    }

    #[test]
    fn test_q0149_02_pasted_secret_is_trimmed_the_same_way_in_both_paths() {
        for slot in SECRET_SLOTS {
            let (via_test, via_save) =
                secret_both_ways(slot, Some(" \u{FEFF}s3cr3t-wert\u{200B}\r\n"), None);

            assert_eq!(
                via_test, via_save,
                "{slot:?}: Verbindungstest und Speichern müssen dasselbe Secret benutzen"
            );
            assert_eq!(
                via_test.as_deref(),
                Ok("s3cr3t-wert"),
                "{slot:?}: Rand-Leerraum, BOM und Zero-Width-Space gehören weggetrimmt"
            );
        }
    }

    #[test]
    fn test_q0149_02_empty_secret_on_update_keeps_the_stored_one_in_both_paths() {
        for slot in SECRET_SLOTS {
            for pasted in EMPTY_INPUTS {
                let (via_test, via_save) =
                    secret_both_ways(slot, Some(pasted), Some("alt-gespeichert"));

                assert_eq!(
                    via_test, via_save,
                    "{slot:?}/{pasted:?}: ein leeres Feld muss in beiden Wegen dasselbe heißen"
                );
                assert_eq!(
                    via_test.as_deref(),
                    Ok("alt-gespeichert"),
                    "{slot:?}/{pasted:?}: leer heißt „das Hinterlegte nehmen\", \
                     nicht „mit leer anmelden\""
                );
            }
        }
    }

    /// Der Kern der Entscheidung zu Q-BL-0149-02: Bei Neuanlage gibt es
    /// nichts zum Wiederverwenden, also endet ein leeres Pflichtfeld in
    /// beiden Wegen im Fehler. Vorher meldete „Verbindung testen" hier auf
    /// einem Server mit `PermitEmptyPasswords yes` einen Erfolg für ein
    /// Profil, das sich anschließend nicht speichern ließ.
    #[test]
    fn test_q0149_02_empty_secret_on_create_is_an_error_in_both_paths() {
        for slot in SECRET_SLOTS {
            for pasted in EMPTY_INPUTS {
                let (via_test, via_save) = secret_both_ways(slot, Some(pasted), None);

                assert!(
                    via_save.is_err(),
                    "{slot:?}/{pasted:?}: Speichern lehnt ein leeres Pflichtfeld ab — \
                     Ausgangslage des Tests, nicht das Prüfziel"
                );
                assert!(
                    via_test.is_err(),
                    "{slot:?}/{pasted:?}: „Verbindung testen\" darf bei Neuanlage mit leerem \
                     Pflichtfeld keinen Anmeldeversuch mit leerem Secret starten, \
                     sondern muss denselben Fehler melden wie das Speichern (war: {via_test:?})"
                );
                // Und zwar aus `resolve_secret`, nicht aus einem beliebigen
                // anderen Grund — und mit dem Code des Speicher-Wegs
                // (Q-BL-0149-03), nicht mit einem Textanfang: Der Code ist
                // die Schnittstelle zum Frontend, der Text nur der Rückfall.
                let code = slot.required_code();
                assert_eq!(
                    via_test.as_ref().err().map(|r| r.code),
                    Some(Some(code)),
                    "{slot:?}/{pasted:?}: der Verbindungstest muss den Code des Speicherns \
                     melden (war: {via_test:?})"
                );
                assert_eq!(
                    via_save.as_ref().err().map(|r| r.code),
                    Some(Some(code)),
                    "{slot:?}/{pasted:?}: Ausgangslage — das Speichern meldet diesen Code"
                );
            }
        }
    }

    /// Q-BL-0149-03: Auch ein **bestehender** Server hilft nicht, wenn seine
    /// Anmeldeart eine andere ist — es gibt für diesen Slot nichts
    /// Hinterlegtes. Der alte Text behauptete hier „kein bestehender Server
    /// gefunden", obwohl es ihn gibt; der Code stimmt in beiden Fällen und
    /// ist derselbe wie beim Speichern.
    #[test]
    fn test_q0149_03_existing_server_with_other_auth_method_and_empty_field_reports_required_code()
    {
        for slot in SECRET_SLOTS {
            let code = slot.required_code();
            let other = slot.other_method();
            // Bestehender Server mit `Agent` (gar kein Secret-Slot) und mit
            // einer anderen Secret-Anmeldeart (Slot vorhanden, aber der
            // falsche).
            let agent: &dyn Fn() -> Option<AuthMethodInput> = &|| Some(AuthMethodInput::Agent);
            let other_secret: &dyn Fn() -> Option<AuthMethodInput> =
                &|| Some(other.input(Some("fremdes-secret")));

            for pasted in EMPTY_INPUTS {
                for (label, existing) in [("Agent", agent), ("andere Anmeldeart", other_secret)] {
                    let (via_test, via_save) = secret_both_ways_from(slot, Some(pasted), existing);

                    assert_eq!(
                        via_save.as_ref().err().map(|r| r.code),
                        Some(Some(code)),
                        "{slot:?}/{label}/{pasted:?}: Ausgangslage — Speichern meldet {code}"
                    );
                    assert_eq!(
                        via_test.as_ref().err().map(|r| r.code),
                        Some(Some(code)),
                        "{slot:?}/{label}/{pasted:?}: der Verbindungstest muss {code} \
                         melden (war: {via_test:?})"
                    );
                }
            }
        }
    }

    /// Die Meldung verrät keinen Wert (I2): auch der hinterlegte Secret-Wert
    /// eines Nachbar-Slots taucht in ihr nicht auf.
    #[test]
    fn test_q0149_03_refusal_message_carries_no_secret() {
        for slot in SECRET_SLOTS {
            let other = slot.other_method();
            let (via_test, _) = secret_both_ways_from(slot, Some("\u{200B}"), &|| {
                Some(other.input(Some("geheimer-nachbarwert")))
            });
            let refusal = via_test.expect_err("leeres Feld ohne passendes Hinterlegtes");
            assert!(
                !refusal.message.contains("geheimer-nachbarwert"),
                "{slot:?}: Meldung darf kein Secret enthalten: {refusal:?}"
            );
        }
    }
}
