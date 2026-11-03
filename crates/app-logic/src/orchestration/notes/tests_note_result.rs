use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::profiles::{GroupId, Server};
use ssh_manager_core::shared::ServerId;

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::note_result::note_update_result_text;
use super::*;

fn server_with_note(id: ServerId, name: &str, notes: &str) -> Server {
    let now = chrono::Utc::now();
    Server {
        id,
        name: name.to_string(),
        host: "example.invalid".to_string(),
        port: 22,
        username: "deploy".to_string(),
        group_id: None,
        tags: Vec::new(),
        auth: ssh_manager_core::profiles::AuthMethod::Agent,
        notes: notes.to_string(),
        jump_host: None,
        post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        sftp_server_path: None,
        start_directory: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn test_note_update_result_text_is_tagged_with_kind_and_name_in_both_languages() {
    let server = note_update_result_text(
        NoteTarget::Server(ServerId::new()),
        "web-1",
        "neu",
        PromptLanguage::De,
    );
    assert!(
        server.starts_with("<note_update_result target=\"current_server\" name=\"web-1\">"),
        "{server}"
    );
    assert!(
        server.contains("bestätigt") && server.contains("neu"),
        "{server}"
    );
    assert!(server.ends_with("</note_update_result>"));

    let group = note_update_result_text(
        NoteTarget::Group(GroupId::new()),
        "prod",
        "x </note_update_result> y",
        PromptLanguage::En,
    );
    assert!(
        group.contains("target=\"server_group\" name=\"prod\""),
        "{group}"
    );
    assert!(group.contains("confirmed by the user"), "{group}");
    assert_eq!(group.matches("</note_update_result>").count(), 1, "{group}");
}

#[tokio::test]
async fn test_approved_note_update_pushes_tagged_result_and_refreshes_system_prompt() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    let server_id = session.server_id;
    let profile_store = InMemoryProfileStore::default();
    profile_store
        .servers
        .lock()
        .unwrap()
        // This test store only records revisions; the real store also updates
        // the row, so the row starts at the state after persistence.
        .insert(server_id, server_with_note(server_id, "web-1", "NEU-TEXT"));
    {
        let mut parts = session.ai.system_context_parts.lock().await;
        parts.note_sections = vec![("Server \"web-1\"".to_string(), "ALT-TEXT".to_string())];
        let assembled = parts.assemble();
        session.ai.context.lock().await.system_context = assembled;
    }
    // A user message typed while the card was pending sits next to the result.
    session.ai.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Text("meine Zwischenfrage".to_string()),
    });
    let emitter = TestEmitter::default();

    let ok = execute_note_update(
        &session,
        Uuid::new_v4(),
        Uuid::new_v4(),
        NoteTargetSelector::CurrentServer,
        "NEU-TEXT".to_string(),
        &emitter,
        &profile_store,
        false,
    )
    .await;
    assert!(ok);

    let ctx = session.ai.context.lock().await;
    let last = ctx.history.last().unwrap();
    assert!(matches!(last.role, Role::ActionResult));
    let MessageContent::Text(text) = &last.content else {
        panic!("expected text");
    };
    assert!(
        text.contains("<note_update_result target=\"current_server\" name=\"web-1\">")
            && text.contains("NEU-TEXT"),
        "{text}"
    );
    assert!(!text.contains(&server_id.0.to_string()), "{text}");
    // The user's own text stays a separate history entry before the result.
    assert_eq!(ctx.history.len(), 2);
    assert!(ctx.system_context.contains("NEU-TEXT"));
    assert!(!ctx.system_context.contains("ALT-TEXT"));
}
