//! Activity record for AI file reads and writes (issue #1446, Spec 0057):
//! path, size, hashes and backup path are recorded, never content or diff.

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};
use ssh_manager_core::filter::{normalize_lexical_path, FilterEngine};
use ssh_manager_core::ssh::mock::MockSftpSession;

use crate::events::TestEmitter;
use crate::orchestration::run_chat_turn;

use super::super::test_support::*;
use super::*;

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn file_entries(
    store: &persistence_sqlite::SqliteLedgerStore,
    chat: Uuid,
) -> Vec<persistence_sqlite::LedgerEntry> {
    store
        .load_entries(chat)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| {
            matches!(
                e.content,
                LedgerEntryContent::FileRead { .. } | LedgerEntryContent::FileWritten { .. }
            )
        })
        .collect()
}

async fn run_write(
    events: Vec<AiEvent>,
    transport: MockSshTransport,
    sftp: MockSftpSession,
    sudo: bool,
    approve: bool,
) -> (
    Vec<persistence_sqlite::LedgerEntry>,
    tempfile::TempDir,
    persistence_sqlite::SqliteChatSessionStore,
) {
    let (mut session, chat_store, chat, tmp, ledger) =
        session_with_real_chat_and_ledger_persistence(events, transport).await;
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowEverythingPolicyStore)));
    if sudo {
        session.set_sudo_password_for_tests(Some(secrecy::SecretString::from("pw".to_string())));
    }
    session.set_sftp_for_tests(Box::new(sftp)).await;
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let turn = run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    );
    if approve {
        let r = approve_first_proposed_action(&emitter, &confirmations);
        tokio::join!(turn, r);
    } else {
        let r = deny_first_proposed_action(&emitter, &confirmations);
        tokio::join!(turn, r);
    }
    (file_entries(&ledger, chat).await, tmp, chat_store)
}

fn write_action(path: &str, content: &str) -> Vec<AiEvent> {
    vec![
        AiEvent::ActionProposed(AiAction::WriteRemoteFile {
            path: path.to_string(),
            content: content.to_string(),
        }),
        AiEvent::Done,
    ]
}

#[tokio::test]
async fn test_file_read_records_path_size_and_hash_without_content() {
    const CONTENT: &[u8] = b"CONTENT-MARKER-4711 hello";
    let (mut session, _cs, chat, _tmp, ledger) = session_with_real_chat_and_ledger_persistence(
        vec![AiEvent::Done],
        MockSshTransport::default(),
    )
    .await;
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowEverythingPolicyStore)));
    session
        .set_sftp_for_tests(Box::new(
            MockSftpSession::new().with_file("/home/deploy/app.conf", CONTENT.to_vec()),
        ))
        .await;
    handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::ReadRemoteFile {
            path: "/home/deploy/app.conf".to_string(),
        },
        &TestEmitter::default(),
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    )
    .await;
    let entries = file_entries(&ledger, chat).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].source, LedgerSource::Ai);
    assert!(entries[0].meta.action_id.is_some());
    let LedgerEntryContent::FileRead {
        path,
        bytes,
        sha256,
    } = &entries[0].content
    else {
        panic!("expected file_read")
    };
    assert_eq!(path, "/home/deploy/app.conf");
    assert_eq!(*bytes, Some(CONTENT.len() as u64));
    assert_eq!(sha256.as_deref(), Some(sha256_hex(CONTENT).as_str()));
    let json = serde_json::to_string(&entries[0].content).unwrap();
    assert!(!json.contains("CONTENT-MARKER"), "{json}");
}

#[tokio::test]
async fn test_file_write_records_backup_and_both_hashes_without_content() {
    let path = "/home/deploy/app.conf";
    let (entries, _t, _c) = run_write(
        write_action(path, "NEW-MARKER-2 body"),
        MockSshTransport::default(),
        MockSftpSession::new().with_file(path, b"OLD-MARKER-1 body".to_vec()),
        false,
        true,
    )
    .await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert!(entries[0].meta.action_id.is_some());
    let LedgerEntryContent::FileWritten {
        path: p,
        backup_path,
        bytes_before,
        bytes_after,
        sha256_before,
        sha256_after,
        sudo,
    } = &entries[0].content
    else {
        panic!("expected file_written")
    };
    assert_eq!(p, path);
    assert!(backup_path
        .as_deref()
        .unwrap()
        .starts_with("/home/deploy/app.conf.smartssh-backup-"));
    assert_eq!(*bytes_before, Some(17));
    assert_eq!(*bytes_after, Some(17));
    assert_eq!(
        sha256_before.as_deref(),
        Some(sha256_hex(b"OLD-MARKER-1 body").as_str())
    );
    assert_eq!(
        sha256_after.as_deref(),
        Some(sha256_hex(b"NEW-MARKER-2 body").as_str())
    );
    assert!(!sudo);
    let json = serde_json::to_string(&entries[0].content).unwrap();
    assert!(!json.contains("MARKER"), "no content in the record: {json}");
}

#[tokio::test]
async fn test_file_write_with_sudo_fallback_records_sudo() {
    let path = "/etc/nginx/nginx.conf";
    let transport = MockSshTransport::default()
        .with_prefix_response("sudo -S cp -p '/etc/nginx/nginx.conf' ", output(""))
        .with_prefix_response("sudo -S install -m 644 ", output(""));
    let (entries, _t, _c) = run_write(
        write_action(path, "neue config"),
        transport,
        MockSftpSession::new()
            .with_file(path, b"alte config".to_vec())
            .with_permission_denied(path),
        true,
        true,
    )
    .await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    let LedgerEntryContent::FileWritten {
        backup_path,
        sha256_after,
        sudo,
        ..
    } = &entries[0].content
    else {
        panic!("expected file_written")
    };
    assert!(sudo);
    assert!(backup_path.is_some());
    assert_eq!(
        sha256_after.as_deref(),
        Some(sha256_hex(b"neue config").as_str())
    );
}

#[tokio::test]
async fn test_rejected_file_write_records_no_file_written_entry() {
    let path = "/home/deploy/app.conf";
    let (entries, _t, _c) = run_write(
        write_action(path, "neu"),
        MockSshTransport::default(),
        MockSftpSession::new().with_file(path, b"alt".to_vec()),
        false,
        false,
    )
    .await;
    assert!(entries.is_empty(), "{entries:?}");
}

#[tokio::test]
async fn test_failed_file_write_records_no_file_written_entry() {
    // Permission denied and no sudo password stored: nothing was written.
    let path = "/etc/nginx/nginx.conf";
    let (entries, _t, _c) = run_write(
        write_action(path, "neu"),
        MockSshTransport::default(),
        MockSftpSession::new()
            .with_file(path, b"alt".to_vec())
            .with_permission_denied(path),
        false,
        true,
    )
    .await;
    assert!(entries.is_empty(), "{entries:?}");
}

#[tokio::test]
async fn test_file_ledger_path_is_redacted_like_command_text() {
    let path = "/srv/app?password=hunter2geheim";
    let normalized = normalize_lexical_path(path);
    let (entries, _t, _c) = run_write(
        write_action(path, "x"),
        MockSshTransport::default(),
        MockSftpSession::new().with_file(normalized.clone(), b"y".to_vec()),
        false,
        true,
    )
    .await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    let json = serde_json::to_string(&entries[0].content).unwrap();
    assert!(!json.contains("hunter2geheim"), "{json}");
}
