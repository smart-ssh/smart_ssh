//! Tests for `dto` (sudo password state).
//!
//! Spec 0071, A14/X4/I4: "unbekannt" ist nicht "nein". Vor diesem
//! Schritt fasste `credential_store.get(...).is_ok()` beide Fälle
//! zusammen — ein nicht erreichbarer Schlüsselbund sah für die
//! Oberfläche genauso aus wie ein Server ohne hinterlegtes
//! Sudo-Passwort, und sie behauptete dann "kein Sudo-Passwort
//! hinterlegt", obwohl sie es nicht wusste.

use super::*;
use crate::server_credentials::sudo_password_credential_ref;
use crate::test_support::InMemoryCredentialStore;
use chrono::Utc;
use ssh_manager_core::profiles::PostIngestPolicy;

fn server() -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: "web-01".to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id: None,
        tags: Vec::new(),
        auth: AuthMethod::Agent,
        notes: String::new(),
        jump_host: None,
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        start_directory: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn test_stored_sudo_password_is_reported_as_present() {
    let server = server();
    let store = InMemoryCredentialStore::new()
        .with_secret(&sudo_password_credential_ref(server.id), "sudo-secret");

    let dto = ServerDto::from_server(&server, &store);

    assert!(dto.has_sudo_password);
    assert!(!dto.sudo_password_unknown);
}

#[test]
fn test_missing_entry_is_reported_as_absent_not_unknown() {
    let server = server();
    let store = InMemoryCredentialStore::new();

    let dto = ServerDto::from_server(&server, &store);

    assert!(!dto.has_sudo_password);
    assert!(
        !dto.sudo_password_unknown,
        "ein fehlender Eintrag ist eine echte Aussage, kein Unwissen"
    );
}

/// Spec 0071, X4 — der eigentliche Regressionstest: Das Sudo-Passwort
/// **ist** hinterlegt, der Schlüsselbund kann es aber nicht sagen. Am
/// ungefixten Stand (`.is_ok()`) meldete das DTO hier
/// `has_sudo_password: false` — eine Behauptung, die es nicht decken
/// kann.
#[test]
fn test_unreadable_keychain_is_reported_as_unknown_not_as_absent() {
    let server = server();
    let store = InMemoryCredentialStore::new()
        .with_secret(&sudo_password_credential_ref(server.id), "sudo-secret")
        .with_failing_get();

    let dto = ServerDto::from_server(&server, &store);

    assert!(
        dto.sudo_password_unknown,
        "ein Backend-Fehler muss als 'unbekannt' erkennbar sein"
    );
    assert!(!dto.has_sudo_password);
}

/// Spec 0071, A17: Dasselbe für das Rückstandsfeld an
/// `DeleteServerResult`. spec-reviewer-Fund: Der Schlüsselname war
/// bisher nur im handgeschriebenen Mock von `ServerForm.test.tsx`
/// festgehalten — ein Rename im Rust-DTO wäre von keinem Test bemerkt
/// worden, und die Oberfläche hätte den Hinweis stillschweigend nicht
/// mehr angezeigt.
#[test]
fn test_secrets_left_behind_is_serialised_as_camel_case() {
    let server = server();
    let store = InMemoryCredentialStore::new();
    let result = DeleteServerResult {
        server: ServerDto::from_server(&server, &store),
        servers_losing_jump_host: Vec::new(),
        executed: true,
        secrets_left_behind: vec!["server:abc:password".to_string()],
    };

    let json = serde_json::to_value(&result).unwrap();

    assert_eq!(
        json["secretsLeftBehind"],
        serde_json::json!(["server:abc:password"])
    );
}

/// Das Feld muss das Frontend auch tatsächlich erreichen — in
/// camelCase, wie alle anderen `ServerDto`-Felder (s. `rename_all`).
#[test]
fn test_unknown_flag_is_serialised_as_camel_case() {
    let server = server();
    let store = InMemoryCredentialStore::new().with_failing_get();

    let json = serde_json::to_value(ServerDto::from_server(&server, &store)).unwrap();

    assert_eq!(json["sudoPasswordUnknown"], serde_json::json!(true));
    assert_eq!(json["hasSudoPassword"], serde_json::json!(false));
}
