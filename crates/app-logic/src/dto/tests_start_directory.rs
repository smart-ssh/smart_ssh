//! Tests for `dto` (start directory input).

use super::{normalize_start_directory_input, ServerDto, ServerInput};

#[test]
fn rejected_paths_are_sanitised_in_error_messages() {
    let err = super::normalize_sftp_server_path(Some("/usr/\u{202E}evil".to_string())).unwrap_err();
    assert!(!err.contains('\u{202E}'), "{err}");
    assert!(err.contains("/usr/?evil"), "{err}");
    let err =
        normalize_start_directory_input(Some("rel\u{202E}\u{2028}x".to_string())).unwrap_err();
    assert!(
        !err.contains('\u{202E}') && !err.contains('\u{2028}'),
        "{err}"
    );
}
use crate::test_support::InMemoryCredentialStore;
use chrono::Utc;
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, Server};
use ssh_manager_core::shared::ServerId;

#[test]
fn missing_or_blank_means_not_set() {
    assert_eq!(normalize_start_directory_input(None), Ok(None));
    assert_eq!(
        normalize_start_directory_input(Some("  ".to_string())),
        Ok(None)
    );
}

#[test]
fn valid_values_are_trimmed() {
    assert_eq!(
        normalize_start_directory_input(Some(" /srv/my app ".to_string())),
        Ok(Some("/srv/my app".to_string()))
    );
    assert_eq!(
        normalize_start_directory_input(Some("~/work".to_string())),
        Ok(Some("~/work".to_string()))
    );
}

#[test]
fn relative_values_are_rejected() {
    for bad in ["srv", "./srv", "~", "~root/x"] {
        assert!(
            normalize_start_directory_input(Some(bad.to_string())).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn server_input_without_field_deserializes_as_not_set() {
    // Ein älterer Frontend-Build schickt das Feld nicht mit.
    let json = serde_json::json!({
        "name": "web",
        "host": "example.org",
        "port": 22,
        "username": "deploy",
        "groupId": null,
        "tags": [],
        "auth": { "kind": "agent" },
        "jumpHost": null,
        "sudoPassword": null
    });
    let input: ServerInput = serde_json::from_value(json).unwrap();
    assert_eq!(input.start_directory, None);
}

#[test]
fn server_dto_carries_start_directory_in_camel_case() {
    let server = Server {
        id: ServerId::new(),
        name: "web".to_string(),
        host: "example.org".to_string(),
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
        start_directory: Some("/srv/app".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let dto = ServerDto::from_server(&server, &InMemoryCredentialStore::new());
    let value = serde_json::to_value(&dto).unwrap();
    assert_eq!(value["startDirectory"], "/srv/app");

    let without = Server {
        start_directory: None,
        ..server
    };
    let value = serde_json::to_value(ServerDto::from_server(
        &without,
        &InMemoryCredentialStore::new(),
    ))
    .unwrap();
    assert!(value["startDirectory"].is_null());
}
