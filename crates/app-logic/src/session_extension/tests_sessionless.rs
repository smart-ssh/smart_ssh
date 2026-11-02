//! Issue #1057: redaction-incomplete notice of the sessionless note summary.

use super::*;
use crate::events::TestEmitter;
use crate::server_credentials::sudo_password_credential_ref;
use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};
use chrono::Utc;
use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, Server};
use std::sync::atomic::Ordering::SeqCst;

const NOTICE: &str = "Some secret values for this server could not be loaded";

struct Ext;
impl SessionExtension for Ext {
    fn known_secrets(&self, _scope: &ServerScope) -> Vec<SecretString> {
        vec![SecretString::from("server-secret-111".to_string())]
    }
}

fn server() -> Server {
    let now = Utc::now();
    Server {
        id: ServerId::new(),
        name: "web".to_string(),
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

/// Builds the redactor and emits the notice like the command does; returns
/// (masked sample, number of notices).
async fn run(
    store: &InMemoryProfileStore,
    credentials: &InMemoryCredentialStore,
    server_id: ServerId,
) -> (String, usize) {
    let sudo = crate::server_redaction::read_sudo_password_checked(credentials, server_id);
    let (redactor, incomplete) =
        redactor_for_sessionless_request(store, sudo, Some(&Ext), server_id, None).await;
    let emitter = TestEmitter::default();
    notify_note_request_redaction_incomplete(&emitter, server_id, incomplete);
    let masked = redactor.redact_text("s=server-secret-111 p=sudo-pw-222");
    let n = emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, p)| {
            name == "note-shrink-failed" && p["message"].as_str().unwrap().contains(NOTICE)
        })
        .count();
    (masked, n)
}

#[tokio::test]
async fn test_failing_get_server_masks_server_secret_and_notices_once() {
    let srv = server();
    let store = InMemoryProfileStore::new().with_server(srv.clone());
    store.fail_get_server.store(true, SeqCst);
    let (masked, n) = run(&store, &InMemoryCredentialStore::new(), srv.id).await;
    assert!(!masked.contains("server-secret-111"), "{masked}");
    assert_eq!(n, 1);
}

#[tokio::test]
async fn test_failing_sudo_read_notices_once() {
    let srv = server();
    let store = InMemoryProfileStore::new().with_server(srv.clone());
    let creds = InMemoryCredentialStore::new()
        .with_secret(&sudo_password_credential_ref(srv.id), "sudo-pw-222")
        .with_failing_get();
    let (_, n) = run(&store, &creds, srv.id).await;
    assert_eq!(n, 1);
}

#[tokio::test]
async fn test_healthy_stores_no_notice_and_sudo_masked() {
    let srv = server();
    let store = InMemoryProfileStore::new().with_server(srv.clone());
    let creds = InMemoryCredentialStore::new()
        .with_secret(&sudo_password_credential_ref(srv.id), "sudo-pw-222");
    let (masked, n) = run(&store, &creds, srv.id).await;
    assert!(!masked.contains("server-secret-111") && !masked.contains("sudo-pw-222"));
    assert_eq!(n, 0);
}

#[tokio::test]
async fn test_server_not_found_counts_as_complete() {
    let (_, n) = run(
        &InMemoryProfileStore::new(),
        &InMemoryCredentialStore::new(),
        ServerId::new(),
    )
    .await;
    assert_eq!(n, 0);
}
