//! Tests for `dto` (identity file DTOs).
//!
//! **Spec 0076, §6.3.2a (B-4):** Liste und Details zeigen den Pfad der
//! Schlüsseldatei; bei jeder anderen Anmeldeart ist das Feld leer.
//!
//! Der Test scheitert, solange das DTO den Pfad nicht trägt — und das
//! ist der Punkt: `auth_kind` allein sagt nur „Schlüsseldatei", nicht
//! **welche**, und der Nutzer könnte in der Oberfläche nicht sehen,
//! woran seine Anmeldung hängt.

use super::*;
use crate::test_support::InMemoryCredentialStore;
use chrono::Utc;
use ssh_manager_core::profiles::PostIngestPolicy;

const PATH: &str = "~/.ssh/id_ed25519";

fn server_with(auth: AuthMethod) -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: "web-01".to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id: None,
        tags: Vec::new(),
        auth,
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
fn test_identity_file_path_is_visible_in_the_dto() {
    let server = server_with(AuthMethod::IdentityFile {
        path: PATH.to_string(),
        passphrase_ref: Some(CredentialRef::new("server:x:passphrase")),
    });

    let dto = ServerDto::from_server(&server, &InMemoryCredentialStore::new());

    assert_eq!(dto.auth_kind, AuthMethodKind::IdentityFile);
    assert_eq!(
        dto.identity_file_path.as_deref(),
        Some(PATH),
        "B-4: ohne den Pfad ist nicht erkennbar, woran die Anmeldung hängt"
    );
}

/// Gegenprobe: Bei jeder anderen Anmeldeart bleibt das Feld leer — es
/// ist ein Feld für **diese** Variante, kein allgemeiner Auth-Kanal.
#[test]
fn test_other_auth_methods_leave_the_identity_file_path_empty() {
    for auth in [
        AuthMethod::Agent,
        AuthMethod::Password {
            credential_ref: CredentialRef::new("p"),
        },
        AuthMethod::PrivateKey {
            credential_ref: CredentialRef::new("k"),
            passphrase_ref: None,
        },
        AuthMethod::Certificate {
            cert_ref: CredentialRef::new("c"),
            key_ref: CredentialRef::new("ck"),
        },
    ] {
        let dto = ServerDto::from_server(&server_with(auth), &InMemoryCredentialStore::new());
        assert_eq!(dto.identity_file_path, None);
    }
}

/// 5.1/B-4: Was **nicht** hinausgeht. Das DTO wird als JSON ans
/// Frontend gereicht — der Pfad darf darin stehen, die Passphrase und
/// deren `CredentialRef` nicht.
#[test]
fn test_the_serialised_dto_carries_the_path_but_no_credential_reference() {
    let server = server_with(AuthMethod::IdentityFile {
        path: PATH.to_string(),
        passphrase_ref: Some(CredentialRef::new("server:secret-slot:passphrase")),
    });

    let json = serde_json::to_string(&ServerDto::from_server(
        &server,
        &InMemoryCredentialStore::new(),
    ))
    .unwrap();

    assert!(json.contains(PATH), "der Pfad gehört hinaus: {json}");
    assert!(
        !json.contains("secret-slot"),
        "ein CredentialRef gehört nicht hinaus: {json}"
    );
    assert!(
        json.contains("identityFilePath"),
        "camelCase fürs Frontend: {json}"
    );
}
