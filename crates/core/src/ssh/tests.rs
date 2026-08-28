//! Testsuite für das SSH-Trait-Modul — reine Logik ohne echtes Netzwerk
//! (Spec 0005, Abschnitt 8, erster Punkt): Jump-Host-Ketten-Auflösung inkl.
//! Zirkelerkennung, Host-Key-Entscheidungslogik gegen einen In-Memory-Store,
//! Fehler-Mapping bei der Auth-Auflösung. Echter Verbindungsaufbau
//! (Integrationstests) ist Sache von `crates/ssh-transport`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};

use super::*;
use crate::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore, Group, GroupId,
    NoteRevision, ProfileError, ProfileResult, ProfileStore, Server,
};
use crate::shared::ServerId;

// --- Test-Only Doubles ------------------------------------------------

#[derive(Default)]
struct MockProfileStore {
    servers: Mutex<HashMap<ServerId, Server>>,
}

impl MockProfileStore {
    fn new() -> Self {
        Self::default()
    }

    fn with_server(self, server: Server) -> Self {
        self.servers.lock().unwrap().insert(server.id, server);
        self
    }
}

#[async_trait]
impl ProfileStore for MockProfileStore {
    async fn get_server(&self, id: &ServerId) -> ProfileResult<Server> {
        self.servers
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or(ProfileError::ServerNotFound(*id))
    }

    async fn list_servers(&self) -> ProfileResult<Vec<Server>> {
        Ok(self.servers.lock().unwrap().values().cloned().collect())
    }

    // Gruppen werden von den ssh-Tests nicht gebraucht — ehrliche, aber
    // triviale Implementierungen statt `unimplemented!()`.
    async fn get_group(&self, id: &GroupId) -> ProfileResult<Group> {
        Err(ProfileError::GroupNotFound(*id))
    }
    async fn create_group(&self, _group: &Group) -> ProfileResult<()> {
        Ok(())
    }
    async fn update_group(&self, _group: &Group) -> ProfileResult<()> {
        Ok(())
    }
    async fn delete_group(&self, _id: &GroupId) -> ProfileResult<()> {
        Ok(())
    }

    async fn create_server(&self, server: &Server) -> ProfileResult<()> {
        self.servers
            .lock()
            .unwrap()
            .insert(server.id, server.clone());
        Ok(())
    }
    async fn update_server(&self, server: &Server) -> ProfileResult<()> {
        self.servers
            .lock()
            .unwrap()
            .insert(server.id, server.clone());
        Ok(())
    }
    async fn delete_server(&self, id: &ServerId) -> ProfileResult<()> {
        self.servers.lock().unwrap().remove(id);
        Ok(())
    }
    async fn record_note_revision(&self, _revision: &NoteRevision) -> ProfileResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct MockCredentialStore {
    values: HashMap<String, SecretString>,
}

impl MockCredentialStore {
    fn new() -> Self {
        Self::default()
    }

    fn with(mut self, key: &str, value: &str) -> Self {
        self.values
            .insert(key.to_string(), SecretString::from(value.to_string()));
        self
    }
}

impl CredentialStore for MockCredentialStore {
    fn get(&self, r: &CredentialRef) -> CredentialResult<SecretString> {
        self.values
            .get(r.as_str())
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(r.clone()))
    }
    fn set(&self, _r: &CredentialRef, _value: SecretString) -> CredentialResult<()> {
        Ok(())
    }
    fn delete(&self, _r: &CredentialRef) -> CredentialResult<()> {
        Ok(())
    }
}

/// In-Memory-`HostKeyStore` (Aufgabenstellung Teil 1, Punkt 3).
#[derive(Default)]
struct InMemoryHostKeyStore {
    known: Mutex<HashMap<(String, u16), Vec<u8>>>,
}

impl InMemoryHostKeyStore {
    fn new() -> Self {
        Self::default()
    }
}

impl HostKeyStore for InMemoryHostKeyStore {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision {
        let known = self.known.lock().unwrap();
        match known.get(&(host.to_string(), port)) {
            None => HostKeyDecision::Unknown {
                fingerprint: hex_fingerprint(key),
            },
            Some(stored) if stored.as_slice() == key => HostKeyDecision::Trusted,
            Some(stored) => HostKeyDecision::Mismatch {
                expected_fingerprint: hex_fingerprint(stored),
                actual_fingerprint: hex_fingerprint(key),
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

fn hex_fingerprint(key: &[u8]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

/// `MockSshTransport` (Aufgabenstellung Teil 1, Punkt 3): konfigurierbar,
/// welche `execute()`-Aufrufe welche `CommandOutput` liefern — für spätere
/// Aufrufer wie eine Filter-Engine-Integration testbar, ohne echtes
/// Netzwerk.
#[derive(Default)]
struct MockSshTransport {
    responses: HashMap<String, CommandOutput>,
    disconnected: bool,
}

impl MockSshTransport {
    fn new() -> Self {
        Self::default()
    }

    fn with_response(mut self, command: impl Into<String>, output: CommandOutput) -> Self {
        self.responses.insert(command.into(), output);
        self
    }
}

#[async_trait]
impl SshTransport for MockSshTransport {
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
        self.responses.get(command).cloned().ok_or_else(|| {
            SshError::ChannelError(format!(
                "kein Mock-Response für Kommando '{command}' konfiguriert"
            ))
        })
    }

    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        Err(SshError::ChannelError(
            "MockSshTransport unterstützt open_shell() nicht (nur für Exec-Modus-Tests gedacht)"
                .to_string(),
        ))
    }

    async fn disconnect(&mut self) -> Result<(), SshError> {
        self.disconnected = true;
        Ok(())
    }
}

// --- Fixtures --------------------------------------------------------

fn make_server(name: &str, jump_host: Option<ServerId>) -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: name.to_string(),
        host: format!("{name}.example.invalid"),
        port: 22,
        username: "deploy".to_string(),
        group_id: None,
        tags: Vec::new(),
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host,
        created_at: now,
        updated_at: now,
    }
}

// --- Jump-Host-Ketten-Auflösung ---------------------------------------

#[tokio::test]
async fn test_jump_host_chain_resolves_multiple_hops_correctly() {
    let bastion = make_server("bastion", None);
    let middle = make_server("middle", Some(bastion.id));
    let target = make_server("target", Some(middle.id));

    let store = MockProfileStore::new()
        .with_server(bastion.clone())
        .with_server(middle.clone())
        .with_server(target.clone());

    let resolved = resolve_connection_target(&target, &store)
        .await
        .expect("keine zyklische Kette in diesem Test");

    let hosts: Vec<&str> = resolved.hops.iter().map(|h| h.host.as_str()).collect();
    assert_eq!(
        hosts,
        vec![
            bastion.host.as_str(),
            middle.host.as_str(),
            target.host.as_str()
        ],
        "erster Hop muss der äußerste Jump-Host sein, letzter das eigentliche Ziel"
    );
    assert_eq!(resolved.hops[0].username, bastion.username);
    assert_eq!(resolved.hops[2].username, target.username);
}

#[tokio::test]
async fn test_jump_host_chain_without_jump_host_is_single_hop() {
    let target = make_server("standalone", None);
    let store = MockProfileStore::new().with_server(target.clone());

    let resolved = resolve_connection_target(&target, &store).await.unwrap();

    assert_eq!(resolved.hops.len(), 1);
    assert_eq!(resolved.hops[0].host, target.host);
}

#[tokio::test]
async fn test_jump_host_cycle_detection_returns_error_not_infinite_loop() {
    let mut a = make_server("a", None);
    let mut b = make_server("b", None);
    // Simuliert einen Store-Fehler: a -> b -> a.
    a.jump_host = Some(b.id);
    b.jump_host = Some(a.id);

    let store = MockProfileStore::new()
        .with_server(a.clone())
        .with_server(b.clone());

    let result = resolve_connection_target(&a, &store).await;
    assert_eq!(result, Err(SshError::JumpHostCycle));
}

// --- Host-Key-Entscheidungslogik ---------------------------------------

#[test]
fn test_host_key_unknown_when_no_stored_key() {
    let store = InMemoryHostKeyStore::new();
    let decision = store.check("example.invalid", 22, b"key-bytes");
    assert_eq!(
        decision,
        HostKeyDecision::Unknown {
            fingerprint: hex_fingerprint(b"key-bytes")
        }
    );
}

#[test]
fn test_host_key_trusted_when_matching_stored_key() {
    let store = InMemoryHostKeyStore::new();
    store.trust("example.invalid", 22, b"key-bytes").unwrap();

    let decision = store.check("example.invalid", 22, b"key-bytes");
    assert_eq!(decision, HostKeyDecision::Trusted);
}

#[test]
fn test_host_key_mismatch_when_key_differs_from_stored() {
    let store = InMemoryHostKeyStore::new();
    store.trust("example.invalid", 22, b"old-key").unwrap();

    let decision = store.check("example.invalid", 22, b"new-key");
    assert_eq!(
        decision,
        HostKeyDecision::Mismatch {
            expected_fingerprint: hex_fingerprint(b"old-key"),
            actual_fingerprint: hex_fingerprint(b"new-key"),
        }
    );
}

#[test]
fn test_host_key_check_is_per_host_and_port() {
    let store = InMemoryHostKeyStore::new();
    store.trust("host-a.invalid", 22, b"key-a").unwrap();

    // Anderer Host, gleicher Key -> trotzdem Unknown (keine Verwechslung).
    assert_eq!(
        store.check("host-b.invalid", 22, b"key-a"),
        HostKeyDecision::Unknown {
            fingerprint: hex_fingerprint(b"key-a")
        }
    );
    // Gleicher Host, anderer Port -> ebenfalls Unknown.
    assert_eq!(
        store.check("host-a.invalid", 2222, b"key-a"),
        HostKeyDecision::Unknown {
            fingerprint: hex_fingerprint(b"key-a")
        }
    );
}

// --- Auth-Auflösung / Fehler-Mapping -----------------------------------

#[test]
fn test_resolve_auth_missing_credential_yields_credential_resolution_failed() {
    let store = MockCredentialStore::new(); // leer, kein Credential hinterlegt
    let auth = AuthMethod::Password {
        credential_ref: CredentialRef::new("missing-cred"),
    };

    let result = resolve_auth(&auth, &store);
    assert!(
        matches!(result, Err(SshError::CredentialResolutionFailed(_))),
        "erwartet CredentialResolutionFailed statt Panic, bekam {result:?}"
    );
}

#[test]
fn test_resolve_auth_missing_passphrase_yields_credential_resolution_failed() {
    let store = MockCredentialStore::new().with("key-ref", "the-private-key-bytes");
    let auth = AuthMethod::PrivateKey {
        credential_ref: CredentialRef::new("key-ref"),
        passphrase_ref: Some(CredentialRef::new("missing-passphrase-ref")),
    };

    let result = resolve_auth(&auth, &store);
    assert!(matches!(
        result,
        Err(SshError::CredentialResolutionFailed(_))
    ));
}

#[test]
fn test_resolve_auth_agent_needs_no_credential_lookup() {
    let store = MockCredentialStore::new();
    let result = resolve_auth(&AuthMethod::Agent, &store);
    assert!(matches!(result, Ok(ResolvedAuth::Agent)));
}

#[test]
fn test_resolve_auth_private_key_with_passphrase_succeeds() {
    let store = MockCredentialStore::new()
        .with("key-ref", "the-private-key-bytes")
        .with("pass-ref", "the-passphrase");
    let auth = AuthMethod::PrivateKey {
        credential_ref: CredentialRef::new("key-ref"),
        passphrase_ref: Some(CredentialRef::new("pass-ref")),
    };

    let result = resolve_auth(&auth, &store).expect("beide Credentials sind hinterlegt");
    match result {
        ResolvedAuth::PrivateKey { key, passphrase } => {
            assert_eq!(key.expose_secret(), "the-private-key-bytes");
            assert_eq!(
                passphrase
                    .expect("Passphrase war konfiguriert")
                    .expose_secret(),
                "the-passphrase"
            );
        }
        other => panic!("expected ResolvedAuth::PrivateKey, got {other:?}"),
    }
}

// --- MockSshTransport ----------------------------------------------------

#[tokio::test]
async fn test_mock_ssh_transport_returns_configured_output() {
    let mut transport = MockSshTransport::new().with_response(
        "echo hi",
        CommandOutput {
            stdout: b"hi\n".to_vec(),
            stderr: Vec::new(),
            exit_code: Some(0),
        },
    );

    let output = transport.execute("echo hi").await.unwrap();
    assert_eq!(output.stdout, b"hi\n");
    assert_eq!(output.exit_code, Some(0));

    transport.disconnect().await.unwrap();
}

#[tokio::test]
async fn test_mock_ssh_transport_errors_on_unconfigured_command_not_panic() {
    let mut transport = MockSshTransport::new();
    let result = transport.execute("whoami").await;
    assert!(matches!(result, Err(SshError::ChannelError(_))));
}
