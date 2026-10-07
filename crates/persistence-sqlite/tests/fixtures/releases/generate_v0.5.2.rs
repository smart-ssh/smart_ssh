//! Generator der Release-Fixture v0.5.2 (Issue #15).
//!
//! **In diesem Baum nicht kompiliert.** Geschrieben gegen die API des Tags
//! v0.5.2 und dort ausgeführt — `v0.5.2.sqlite3` ist also eine Datei, die
//! der Code dieses Release selbst geschrieben hat (Klartext, Migrationen
//! 1–14, Chat-Schlüssel ist `RELEASE_FIXTURE_ROOT_KEY`). Liegt hier als
//! Nachweis, was die Fixture enthält, und als Vorlage für den Generator des
//! nächsten Release — s. `README.md` in diesem Verzeichnis.

use std::sync::Arc;

use ssh_manager_core::ai::{ChatMessage, MessageContent, ProviderId, ProviderType, Role};
use ssh_manager_core::audit::{LedgerDecisionOutcome, LedgerEntryContent, LedgerSource};
use ssh_manager_core::crypto::{ChaCha20Poly1305Cipher, ContentCipher};
use ssh_manager_core::filter::{Pattern, RuleAction, RuleId, RuleOrigin, Scope};
use ssh_manager_core::profiles::{
    AuthMethod, CredentialRef, Group, GroupId, NoteEditor, NoteRevision, NoteTarget,
    PostIngestPolicy, ProfileStore, Server,
};
use ssh_manager_core::shared::ServerId;
use uuid::Uuid;

use crate::{AiProviderConfig, SqliteProfileStore, StoredRule};

const KEY: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

#[tokio::test]
#[ignore]
async fn generate_release_fixture() {
    let out = std::path::PathBuf::from(std::env::var("FIXTURE_OUT").expect("FIXTURE_OUT"));
    assert!(!out.exists());
    let store = SqliteProfileStore::connect(&out).await.unwrap();
    let cipher: Arc<dyn ContentCipher> = Arc::new(ChaCha20Poly1305Cipher::new(&KEY));
    let ts = |s: &str| {
        chrono::DateTime::parse_from_rfc3339(s)
            .unwrap()
            .with_timezone(&chrono::Utc)
    };

    let parent = GroupId(Uuid::from_u128(0x0001));
    let child = GroupId(Uuid::from_u128(0x0002));
    let server_a = ServerId(Uuid::from_u128(0x0011));
    let server_b = ServerId(Uuid::from_u128(0x0012));
    let provider = ProviderId(Uuid::from_u128(0x0021));

    store
        .create_group(&Group {
            id: parent,
            name: "Release fixture parent".into(),
            parent_id: None,
            notes: "group-note-r052".into(),
            created_at: ts("2026-10-01T10:00:00Z"),
            updated_at: ts("2026-10-01T10:00:00Z"),
        })
        .await
        .unwrap();
    store
        .create_group(&Group {
            id: child,
            name: "Release fixture child".into(),
            parent_id: Some(parent),
            notes: String::new(),
            created_at: ts("2026-10-01T10:01:00Z"),
            updated_at: ts("2026-10-01T10:01:00Z"),
        })
        .await
        .unwrap();
    store
        .create_server(&Server {
            id: server_a,
            name: "Release fixture A".into(),
            host: "host-a-r052.example".into(),
            port: 2222,
            username: "user-a-r052".into(),
            group_id: Some(child),
            tags: vec!["db".into(), "prod-r052".into()],
            auth: AuthMethod::Password {
                credential_ref: CredentialRef::new(format!("server:{}:password", server_a.0)),
            },
            notes: "server-note-r052".into(),
            jump_host: None,
            post_ingest_policy: PostIngestPolicy::Strict,
            ai_injection_check_enabled: true,
            sftp_server_path: Some("/usr/lib/sftp-r052".into()),
            created_at: ts("2026-10-01T10:02:00Z"),
            updated_at: ts("2026-10-01T10:02:00Z"),
        })
        .await
        .unwrap();
    store
        .create_server(&Server {
            id: server_b,
            name: "Release fixture B".into(),
            host: "host-b-r052.example".into(),
            port: 22,
            username: "user-b-r052".into(),
            group_id: None,
            tags: Vec::new(),
            auth: AuthMethod::PrivateKey {
                credential_ref: CredentialRef::new(format!("server:{}:private_key", server_b.0)),
                passphrase_ref: Some(CredentialRef::new(format!(
                    "server:{}:passphrase",
                    server_b.0
                ))),
            },
            notes: String::new(),
            jump_host: Some(server_a),
            post_ingest_policy: PostIngestPolicy::Balanced,
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            created_at: ts("2026-10-01T10:03:00Z"),
            updated_at: ts("2026-10-01T10:03:00Z"),
        })
        .await
        .unwrap();
    store
        .record_note_revision(&NoteRevision {
            id: Uuid::from_u128(0x0031),
            target: NoteTarget::Server(server_a),
            content: "server-note-r052".into(),
            edited_by: NoteEditor::User,
            created_at: ts("2026-10-01T10:04:00Z"),
        })
        .await
        .unwrap();
    store
        .record_note_revision(&NoteRevision {
            id: Uuid::from_u128(0x0032),
            target: NoteTarget::Group(parent),
            content: "group-note-r052".into(),
            edited_by: NoteEditor::Ai {
                provider: "anthropic".into(),
                model: "model-r052".into(),
            },
            created_at: ts("2026-10-01T10:05:00Z"),
        })
        .await
        .unwrap();

    let policy = store.policy_store();
    for (id, pattern, action, scope, priority) in [
        (
            "rule-r052-deny",
            Pattern::Regex(r"rm\s+-rf\s+/".into()),
            RuleAction::Deny,
            Scope::Global,
            100,
        ),
        (
            "rule-r052-confirm",
            Pattern::Glob("systemctl restart *".into()),
            RuleAction::Confirm,
            Scope::Tag("prod-r052".into()),
            50,
        ),
        (
            "rule-r052-allow",
            Pattern::Exact("uptime".into()),
            RuleAction::Allow,
            Scope::Server(server_a),
            10,
        ),
    ] {
        policy
            .create(&StoredRule {
                id: RuleId(id.into()),
                pattern,
                action,
                scope,
                priority,
                created_at: ts("2026-10-01T10:06:00Z"),
                updated_at: ts("2026-10-01T10:06:00Z"),
            })
            .await
            .unwrap();
    }

    store
        .ai_provider_store()
        .create(&AiProviderConfig {
            id: provider,
            provider_type: ProviderType::Anthropic,
            display_name: "Release fixture provider".into(),
            base_url: Some("https://api-r052.example".into()),
            model: "model-r052".into(),
            supports_native_tool_calling: true,
            credential_ref: CredentialRef::new(format!("ai-provider:{}", provider.0)),
            is_active: true,
            extra_headers: vec![("X-Fixture".into(), "Header-r052".into())],
            attestation_url: None,
            max_tokens_override: Some(4096),
            created_at: ts("2026-10-01T10:07:00Z"),
            updated_at: ts("2026-10-01T10:07:00Z"),
        })
        .await
        .unwrap();
    // `create` never activates a provider (Spec 0007); the app does it
    // through `set_active`, so the fixture does too.
    store
        .ai_provider_store()
        .set_active(&provider, ts("2026-10-01T10:08:00Z"))
        .await
        .unwrap();

    let chat = store.chat_session_store(cipher.clone());
    let session = chat
        .create_session(&server_a, Some(provider.0))
        .await
        .unwrap();
    chat.append_message(
        session,
        &ChatMessage {
            role: Role::User,
            content: MessageContent::Text("chat-user-r052".into()),
        },
    )
    .await
    .unwrap();
    chat.append_message(
        session,
        &ChatMessage {
            role: Role::Assistant,
            content: MessageContent::Text("chat-assistant-r052".into()),
        },
    )
    .await
    .unwrap();
    chat.set_title_if_absent(session, "title-r052").await.unwrap();
    chat.save_summary(session, "summary-r052", 1).await.unwrap();
    chat.mark_ended(session).await.unwrap();

    let history = store.prompt_history_store(cipher.clone());
    history.record(&server_a, "prompt-r052-1").await.unwrap();
    history.record(&server_a, "prompt-r052-2").await.unwrap();

    let ledger = store.ledger_store(cipher);
    ledger
        .append_entry(
            session,
            LedgerSource::User,
            &LedgerEntryContent::CommandProposed {
                command: "uptime-r052".into(),
            },
        )
        .await
        .unwrap();
    ledger
        .append_entry(
            session,
            LedgerSource::User,
            &LedgerEntryContent::Decision {
                outcome: LedgerDecisionOutcome::Confirmed,
                reason: Some("reason-r052".into()),
                code: None,
                matched_rule: Some(RuleId("rule-r052-allow".into())),
                matched_rule_origin: Some(RuleOrigin::User),
            },
        )
        .await
        .unwrap();

    store.pool.close().await;
    for suffix in ["-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", out.display()));
    }
}
