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

use super::mock::MockKeyFileReader;
use super::*;
use crate::profiles::{
    AuthMethod, CredentialError, CredentialRef, CredentialResult, CredentialStore, Group, GroupId,
    NoteRevision, PostIngestPolicy, ProfileError, ProfileResult, ProfileStore, Server,
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
    async fn list_groups(&self) -> ProfileResult<Vec<Group>> {
        Ok(Vec::new())
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
    async fn list_note_revisions(
        &self,
        _target: crate::profiles::NoteTarget,
    ) -> ProfileResult<Vec<NoteRevision>> {
        Ok(Vec::new())
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
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
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

/// Spec 0076, §4.2: Für die vier Anmeldearten, die keine Datei anfassen,
/// ist die Attrappe leer — ein Zugriff darauf wäre ein Fehler und fiele als
/// „nicht gefunden" auf, statt still zu gelingen.
fn no_key_files() -> crate::ssh::mock::MockKeyFileReader {
    crate::ssh::mock::MockKeyFileReader::new()
}

const IDENTITY_PATH: &str = "/home/deploy/.ssh/id_ed25519";

/// §6.1.1: `IdentityFile` löst zu `ResolvedAuth::PrivateKey` auf — mit
/// **genau** dem Inhalt der Attrappe. Jede andere Variante fällt sofort
/// auf; ein vertauschter Inhalt ebenso.
#[test]
fn test_resolve_auth_identity_file_yields_private_key_with_file_content() {
    let store = MockCredentialStore::new();
    let key_files = MockKeyFileReader::new().with_key(IDENTITY_PATH, "key-from-the-file");
    let auth = AuthMethod::IdentityFile {
        path: IDENTITY_PATH.to_string(),
        passphrase_ref: None,
    };

    let result = resolve_auth(&auth, &store, &key_files).expect("die Datei ist lesbar");

    match result {
        ResolvedAuth::PrivateKey { key, passphrase } => {
            assert_eq!(key.expose_secret(), "key-from-the-file");
            assert!(
                passphrase.is_none(),
                "ohne passphrase_ref darf keine Passphrase entstehen"
            );
        }
        other => panic!("A-2 verlangt ResolvedAuth::PrivateKey, bekam {other:?}"),
    }
}

/// §6.1.2: Die Passphrase kommt aus dem `CredentialStore`, nicht aus der
/// Datei — die Attrappe liefert hier bewusst einen Dateiinhalt, der sich
/// vom Passphrase-Wert unterscheidet.
#[test]
fn test_resolve_auth_identity_file_takes_passphrase_from_the_credential_store() {
    let store = MockCredentialStore::new().with("pass-ref", "the-passphrase");
    let key_files = MockKeyFileReader::new().with_encrypted_key(IDENTITY_PATH, "encrypted-key");
    let auth = AuthMethod::IdentityFile {
        path: IDENTITY_PATH.to_string(),
        passphrase_ref: Some(CredentialRef::new("pass-ref")),
    };

    let result = resolve_auth(&auth, &store, &key_files).expect("Datei und Passphrase liegen vor");

    match result {
        ResolvedAuth::PrivateKey { key, passphrase } => {
            assert_eq!(key.expose_secret(), "encrypted-key");
            assert_eq!(
                passphrase
                    .expect("die Passphrase war hinterlegt")
                    .expose_secret(),
                "the-passphrase"
            );
        }
        other => panic!("erwartet ResolvedAuth::PrivateKey, bekam {other:?}"),
    }
}

/// §6.3.8 auf Auflösungsebene, und der eigentliche Prüfpunkt von A-5:
/// **Ein verschlüsselter Schlüssel mit hinterlegter Passphrase verbindet.**
///
/// Der Test scheitert, sobald jemand die Passphrase-Entscheidung ins Lesen
/// verlegt (also `KeyFileReader::read` bei `encrypted == true` schon mit
/// einem Fehler enden lässt): Die Datei weiß nichts von `passphrase_ref`,
/// und jeder verschlüsselte Schlüssel wäre damit unbenutzbar — auch der
/// mit korrekt hinterlegter Passphrase.
#[test]
fn test_resolve_auth_encrypted_identity_file_with_stored_passphrase_resolves() {
    let store = MockCredentialStore::new().with("pass-ref", "correct horse");
    let key_files = MockKeyFileReader::new().with_encrypted_key(IDENTITY_PATH, "encrypted-key");
    let auth = AuthMethod::IdentityFile {
        path: IDENTITY_PATH.to_string(),
        passphrase_ref: Some(CredentialRef::new("pass-ref")),
    };

    let result = resolve_auth(&auth, &store, &key_files);

    assert!(
        matches!(result, Ok(ResolvedAuth::PrivateKey { .. })),
        "ein verschlüsselter Schlüssel mit hinterlegter Passphrase muss auflösen, bekam \
         {result:?}"
    );
}

/// A-5/A-6: **ohne** hinterlegte Passphrase ergibt derselbe Schlüssel eine
/// Meldung, die genau das sagt — und den Pfad nennt (nur `resolve_auth`
/// kennt ihn, §4.2). Nicht „Anmeldung fehlgeschlagen".
#[test]
fn test_resolve_auth_encrypted_identity_file_without_passphrase_names_path_and_reason() {
    let store = MockCredentialStore::new();
    let key_files = MockKeyFileReader::new().with_encrypted_key(IDENTITY_PATH, "encrypted-key");
    let auth = AuthMethod::IdentityFile {
        path: IDENTITY_PATH.to_string(),
        passphrase_ref: None,
    };

    let err = resolve_auth(&auth, &store, &key_files)
        .expect_err("ohne Passphrase ist ein verschlüsselter Schlüssel nicht benutzbar");

    let SshError::CredentialResolutionFailed(message) = &err else {
        panic!("A-7 verlangt CredentialResolutionFailed, bekam {err:?}");
    };
    assert!(
        message.contains(IDENTITY_PATH),
        "die Meldung muss den Pfad nennen: {message}"
    );
    assert!(
        message.contains("Passphrase"),
        "die Meldung muss den Grund nennen: {message}"
    );
    assert!(
        !message.contains("encrypted-key"),
        "5.2: kein Dateiinhalt in der Meldung: {message}"
    );
}

/// §6.1.3: Ein Lesefehler wird zu [`SshError::CredentialResolutionFailed`]
/// — kein neuer Fehlertyp, kein Panic (A-7).
#[test]
fn test_resolve_auth_identity_file_missing_file_yields_credential_resolution_failed() {
    let store = MockCredentialStore::new();
    // Die Attrappe kennt den Pfad gar nicht → „nicht gefunden".
    let key_files = MockKeyFileReader::new();
    let auth = AuthMethod::IdentityFile {
        path: IDENTITY_PATH.to_string(),
        passphrase_ref: None,
    };

    let result = resolve_auth(&auth, &store, &key_files);

    assert!(
        matches!(result, Err(SshError::CredentialResolutionFailed(_))),
        "erwartet CredentialResolutionFailed statt Panic, bekam {result:?}"
    );
}

/// §6.1.5: Jeder Fehlerfall aus A-6 ist **eine unterscheidbare Variante**,
/// geprüft gegen die Variante statt gegen den Meldungstext. Ein Test gegen
/// Zeichenketten wäre kaum zum Scheitern zu bringen und würde beim
/// nächsten Umformulieren rot.
///
/// Zusätzlich: Jede Meldung, die daraus entsteht, nennt den Pfad — das ist
/// der Grund, warum §4.2 jede Variante den Pfad tragen lässt.
#[test]
fn test_every_key_file_error_is_a_distinct_variant_carrying_its_path() {
    let path = IDENTITY_PATH.to_string();
    let all = [
        KeyFileError::NotFound { path: path.clone() },
        KeyFileError::NotReadable { path: path.clone() },
        KeyFileError::PermissionsTooOpen {
            path: path.clone(),
            mode: 0o644,
        },
        KeyFileError::TooLarge {
            path: path.clone(),
            limit: MAX_KEY_FILE_BYTES,
        },
        KeyFileError::NotARegularFile { path: path.clone() },
        KeyFileError::PathNotAbsolute { path: path.clone() },
        KeyFileError::InvalidKey { path: path.clone() },
    ];

    let mut codes: Vec<&'static str> = all.iter().map(KeyFileError::code).collect();
    let total = codes.len();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(total, codes.len(), "doppelt vergebener KeyFileError-Code");

    for err in &all {
        assert_eq!(err.path(), IDENTITY_PATH);
        let rendered = err.to_string();
        assert!(
            rendered.contains(IDENTITY_PATH),
            "A-6: jede Meldung nennt den Pfad — {rendered}"
        );
    }

    // A-4/A-6: die Rechte-Meldung nennt zusätzlich den Befehl, der es
    // behebt. Ohne ihn wüsste der Nutzer, dass etwas klemmt, aber nicht,
    // was zu tun ist.
    let rights = KeyFileError::PermissionsTooOpen {
        path: path.clone(),
        mode: 0o644,
    }
    .to_string();
    assert!(
        rights.contains(&format!("chmod 600 {IDENTITY_PATH}")),
        "die Meldung muss den chmod-Befehl nennen: {rights}"
    );
}

/// §6.1.5, Gegenrichtung: Jeder Fehler der Attrappe landet **unverändert
/// als Variante** bei `resolve_auth` und wird dort zu genau einem
/// `CredentialResolutionFailed` — nie zu einem stillen Erfolg.
#[test]
fn test_each_key_file_error_reaches_resolve_auth_as_a_failure() {
    let path = IDENTITY_PATH.to_string();
    let cases = [
        KeyFileError::NotFound { path: path.clone() },
        KeyFileError::NotReadable { path: path.clone() },
        KeyFileError::PermissionsTooOpen {
            path: path.clone(),
            mode: 0o644,
        },
        KeyFileError::TooLarge {
            path: path.clone(),
            limit: MAX_KEY_FILE_BYTES,
        },
        KeyFileError::NotARegularFile { path: path.clone() },
        KeyFileError::PathNotAbsolute { path: path.clone() },
        KeyFileError::InvalidKey { path: path.clone() },
    ];

    for case in cases {
        let store = MockCredentialStore::new();
        let key_files = MockKeyFileReader::new().with_error(IDENTITY_PATH, case.clone());
        let auth = AuthMethod::IdentityFile {
            path: path.clone(),
            passphrase_ref: None,
        };

        let result = resolve_auth(&auth, &store, &key_files);
        let Err(SshError::CredentialResolutionFailed(message)) = result else {
            panic!("{case:?} muss zu CredentialResolutionFailed führen, bekam {result:?}");
        };
        assert!(
            message.contains(IDENTITY_PATH),
            "{case:?}: die Meldung muss den Pfad nennen — {message}"
        );
    }
}

/// A-4/§4.2: Der Anmeldepfad ruft `read` mit `enforce_permissions == true`.
/// Genau daran hängt, ob eine zu weit geöffnete Datei die **Anmeldung**
/// sperrt — die Überführung (C-3) ruft dieselbe Methode mit `false`.
///
/// Der Test scheitert, sobald jemand den Schalter auf dem Anmeldepfad auf
/// `false` setzt: Dann läge die Datei offen und die Anmeldung liefe
/// trotzdem durch.
#[test]
fn test_resolve_auth_reads_with_permission_enforcement_enabled() {
    let store = MockCredentialStore::new();
    let key_files = MockKeyFileReader::new().with_key(IDENTITY_PATH, "key");
    let auth = AuthMethod::IdentityFile {
        path: IDENTITY_PATH.to_string(),
        passphrase_ref: None,
    };

    resolve_auth(&auth, &store, &key_files).expect("die Datei ist lesbar");

    assert_eq!(
        key_files.calls(),
        vec![format!("read {IDENTITY_PATH} true")],
        "A-4: der Anmeldepfad liest mit erzwungener Rechteprüfung — und nur einmal"
    );
}

/// E-5/§4.3: **Bei jedem** Verbindungsaufbau wird neu gelesen, nichts wird
/// zwischengespeichert. Ein getauschter Schlüssel wirkt damit sofort, und
/// nichts liegt länger im Speicher als nötig.
///
/// Der Test scheitert, sobald jemand einen Zwischenspeicher einzieht.
#[test]
fn test_resolve_auth_reads_the_file_on_every_resolution() {
    let store = MockCredentialStore::new();
    let key_files = MockKeyFileReader::new().with_key(IDENTITY_PATH, "first");
    let auth = AuthMethod::IdentityFile {
        path: IDENTITY_PATH.to_string(),
        passphrase_ref: None,
    };

    resolve_auth(&auth, &store, &key_files).expect("erster Aufbau");
    // Der Schlüssel wird ausgetauscht, wie es ein Nutzer täte.
    let key_files = key_files.with_key(IDENTITY_PATH, "second");
    let result = resolve_auth(&auth, &store, &key_files).expect("zweiter Aufbau");

    assert_eq!(key_files.read_calls(), 2, "E-5: kein Zwischenspeicher");
    match result {
        ResolvedAuth::PrivateKey { key, .. } => assert_eq!(
            key.expose_secret(),
            "second",
            "der zweite Aufbau muss den neuen Schlüssel benutzen"
        ),
        other => panic!("erwartet ResolvedAuth::PrivateKey, bekam {other:?}"),
    }
}

#[test]
fn test_resolve_auth_missing_credential_yields_credential_resolution_failed() {
    let store = MockCredentialStore::new(); // leer, kein Credential hinterlegt
    let auth = AuthMethod::Password {
        credential_ref: CredentialRef::new("missing-cred"),
    };

    let result = resolve_auth(&auth, &store, &no_key_files());
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

    let result = resolve_auth(&auth, &store, &no_key_files());
    assert!(matches!(
        result,
        Err(SshError::CredentialResolutionFailed(_))
    ));
}

#[test]
fn test_resolve_auth_agent_needs_no_credential_lookup() {
    let store = MockCredentialStore::new();
    let result = resolve_auth(&AuthMethod::Agent, &store, &no_key_files());
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

    let result =
        resolve_auth(&auth, &store, &no_key_files()).expect("beide Credentials sind hinterlegt");
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

/// **§6.4.8 — keine Verzweigung in der Sicherheitslogik (5.6).**
///
/// Ein Kommando mit rotem Risiko wird auf einem Server mit
/// Schlüsseldatei-Anmeldung genauso behandelt wie auf einem mit
/// gespeichertem Schlüssel. Geprüft an den drei Stellen, an denen eine
/// Verzweigung überhaupt entstehen könnte:
///
/// 1. **Risikoeinstufung** — sie kennt nur das Kommando; dieselbe
///    Einstufung für beide Server.
/// 2. **Filter-Engine** — dieselbe Regel, dieselbe Entscheidung. Beide
///    Server tragen hier bewusst **dieselbe** `ServerId`: Es ist derselbe
///    Server, nur anders angemeldet. Käme trotzdem eine andere
///    Entscheidung heraus, hinge sie an der Anmeldeart.
/// 3. **Der Auswertungskontext selbst** — die letzte Zeile ist ein
///    Compile-Wächter: Sobald jemand `EvalContext` ein Feld hinzufügt
///    (etwa die Anmeldeart), übersetzt sie nicht mehr. Ohne ihn könnte
///    eine künftige Verzweigung still einziehen.
#[tokio::test]
async fn test_t6_4_8_security_logic_does_not_branch_on_the_auth_method() {
    use crate::filter::{
        Decision, EffectiveScope, EvalContext, FilterEngine, Pattern, PolicyStore, Rule,
        RuleAction, RuleId, RuleOrigin, Scope,
    };
    use crate::risk::{RiskClassifier, RiskLevel, RuleBasedRiskClassifier};

    struct OneRule(Rule);
    #[async_trait]
    impl PolicyStore for OneRule {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![self.0.clone()]
        }
    }

    const RED_COMMAND: &str = "rm -rf /";
    let shared_id = ServerId::new();

    let stored_key = AuthMethod::PrivateKey {
        credential_ref: CredentialRef::new("key-ref"),
        passphrase_ref: None,
    };
    let from_file = AuthMethod::IdentityFile {
        path: IDENTITY_PATH.to_string(),
        passphrase_ref: None,
    };
    assert_ne!(stored_key, from_file, "zwei verschiedene Anmeldearten");

    // 1. Risikoeinstufung — rot bleibt rot.
    let classifier = RuleBasedRiskClassifier;
    let assessment = classifier.classify(RED_COMMAND);
    assert_eq!(
        assessment.server_risk,
        RiskLevel::Red,
        "der Testfall taugt nur mit einem tatsächlich roten Kommando"
    );

    // 2. Filter-Engine — dieselbe Regel, dieselbe Entscheidung.
    let engine = FilterEngine::new(OneRule(Rule {
        id: RuleId("deny-rm".to_string()),
        pattern: Pattern::Glob("rm *".to_string()),
        action: RuleAction::Deny,
        scope: Scope::Server(shared_id),
        priority: 0,
        origin: RuleOrigin::User,
    }));

    let ctx_stored = EvalContext {
        server_id: shared_id,
        tags: vec!["prod".to_string()],
    };
    let ctx_from_file = EvalContext {
        server_id: shared_id,
        tags: vec!["prod".to_string()],
    };

    let decision_stored = engine.evaluate(RED_COMMAND, &ctx_stored).await;
    let decision_from_file = engine.evaluate(RED_COMMAND, &ctx_from_file).await;

    assert!(
        matches!(decision_stored, Decision::Deny { .. }),
        "der Testfall taugt nur, wenn die Regel überhaupt greift, bekam {decision_stored:?}"
    );
    assert_eq!(
        decision_stored, decision_from_file,
        "5.6: die Filterentscheidung darf nicht an der Anmeldeart hängen"
    );

    // 3. Compile-Wächter: Der Auswertungskontext trägt genau diese zwei
    //    Felder — Anmeldeart, Pfad und Schlüsselmaterial gehören nicht
    //    hinein und können dort deshalb auch nicht ausgewertet werden.
    let EvalContext {
        server_id: _,
        tags: _,
    } = ctx_from_file;
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
            truncated: false,
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
