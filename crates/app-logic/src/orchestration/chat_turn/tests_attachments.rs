//! Issue #469: Dateianhänge im Chat-Turn — Redaction vor dem Versand,
//! Speicherung, Spec-0039-§5-Eskalation, Tokenschätzung.

use uuid::Uuid;

use ssh_manager_core::ai::{AiEvent, AttachmentKind, ChatAttachment};
use ssh_manager_core::profiles::{AiAction, PostIngestPolicy};

use crate::events::TestEmitter;

use super::super::attachments::push_user_attachments;
use super::super::test_support::*;
use super::*;

const PLANTED_KEY: &str = "AKIAABCDEFGHIJKLMNOP";

fn text_attachment(name: &str, body: &str) -> ChatAttachment {
    ChatAttachment {
        name: name.to_string(),
        media_type: "text/plain".to_string(),
        kind: AttachmentKind::Text,
        data: body.as_bytes().to_vec(),
        byte_len: body.len() as u64,
    }
}

/// Debug output prints `Vec<u8>` as numbers, so decode the text of every
/// attachment explicitly before searching for a planted secret.
fn attachment_texts(history: &[ChatMessage]) -> String {
    history
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::Attachments(items) => Some(
                items
                    .iter()
                    .map(|a| format!("{} {}", a.name, a.text()))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn run_turn(session: &Session) {
    run_chat_turn(
        session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;
}

fn done_provider() -> MockAiProvider {
    MockAiProvider::with_rounds(vec![vec![AiEvent::TextDelta("ok".into()), AiEvent::Done]])
}

/// A secret inside an attachment never reaches the provider request, neither
/// when pushed through the normal path nor when an unredacted attachment is
/// already in the history (resume of a session stored with an older pattern
/// set): redaction runs again on every send.
#[tokio::test]
async fn test_secret_in_text_attachment_never_reaches_provider() {
    let provider = done_provider();
    let sent = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let body = format!("config\nkey = {PLANTED_KEY}\n");

    // Resume-like: unredacted attachment straight into the history.
    session.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Attachments(vec![text_attachment("a.env", &body)]),
    });
    // Normal path.
    push_user_attachments(&session, vec![text_attachment("b.env", &body)])
        .await
        .unwrap();
    // The stored (in-memory) copy from the normal path is already redacted.
    let stored = attachment_texts(&session.context.lock().await.history[1..2]);
    assert!(
        stored.contains("config") && !stored.contains(PLANTED_KEY),
        "{stored}"
    );

    run_turn(&session).await;

    let contexts = sent.lock().unwrap().clone();
    assert_eq!(contexts.len(), 1);
    let request = attachment_texts(&contexts[0].history);
    assert!(request.contains("config"), "attachments must still be sent");
    assert!(!request.contains(PLANTED_KEY), "{request}");
}

/// Binary attachments are passed through untouched (never interpreted).
#[tokio::test]
async fn test_binary_attachment_bytes_are_not_modified_by_redaction() {
    let session = session_with_ai_provider(done_provider(), MockSshTransport::default());
    let png = ChatAttachment {
        name: "x.png".into(),
        media_type: "image/png".into(),
        kind: AttachmentKind::Image,
        data: vec![0, 1, 2, 255],
        byte_len: 4,
    };
    push_user_attachments(&session, vec![png.clone()])
        .await
        .unwrap();
    let history = session.context.lock().await.history.clone();
    assert_eq!(history[0].content, MessageContent::Attachments(vec![png]));
}

/// Issue #470: a secret in a text attachment of the same message as an image
/// is still redacted before sending, while the image bytes arrive unchanged.
#[tokio::test]
async fn test_secret_next_to_image_is_redacted_and_image_bytes_unchanged() {
    let provider = done_provider();
    let sent = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let png = ChatAttachment {
        name: "shot.png".into(),
        media_type: "image/png".into(),
        kind: AttachmentKind::Image,
        data: vec![137, 80, 78, 71, 0, 255],
        byte_len: 6,
    };
    let body = format!("key = {PLANTED_KEY}\n");
    session.context.lock().await.history.push(ChatMessage {
        role: Role::User,
        content: MessageContent::Attachments(vec![text_attachment("a.env", &body), png.clone()]),
    });

    run_turn(&session).await;

    let contexts = sent.lock().unwrap().clone();
    let request = attachment_texts(&contexts[0].history);
    assert!(
        request.contains("a.env") && !request.contains(PLANTED_KEY),
        "{request}"
    );
    let MessageContent::Attachments(items) = &contexts[0].history[0].content else {
        panic!("attachments expected");
    };
    assert_eq!(items[1], png);
}

/// Spec 0039 §5: after an attachment, an AI-proposed command is escalated
/// to confirmation (Strict policy); without the attachment it is not. The
/// second half is the counter-proof.
#[tokio::test]
async fn test_action_escalated_after_attachment_but_not_without() {
    let build = || {
        let mut session = session_with_ai_provider(
            done_provider(),
            MockSshTransport::default().with_response("echo hi", output("hi")),
        );
        session.set_filter_engine_for_tests(Box::new(ssh_manager_core::filter::FilterEngine::new(
            AllowEverythingPolicyStore,
        )));
        session.set_post_ingest_policy_for_tests(PostIngestPolicy::Strict);
        session
    };
    let code = |session: Session| async move {
        let (_, payload) = proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "echo hi".to_string(),
            },
        )
        .await;
        payload["decision"].clone()
    };

    let with = build();
    push_user_attachments(&with, vec![text_attachment("n.txt", "hello")])
        .await
        .unwrap();
    let decision = code(with).await;
    assert_eq!(
        decision["Confirm"]["code"],
        serde_json::json!("FILTER_POST_INGEST_REQUIRES_CONFIRM"),
        "{decision}"
    );

    let without = build();
    let decision = code(without).await;
    assert_ne!(
        decision["Confirm"]["code"],
        serde_json::json!("FILTER_POST_INGEST_REQUIRES_CONFIRM"),
        "{decision}"
    );
}

/// A resumed history containing attachments counts as untrusted (§5).
#[test]
fn test_resumed_history_with_attachments_is_untrusted() {
    let history = vec![ChatMessage {
        role: Role::User,
        content: MessageContent::Attachments(vec![text_attachment("n.txt", "x")]),
    }];
    assert!(crate::session::history_contains_untrusted_content(&history));
    assert!(!crate::session::history_contains_untrusted_content(&[]));
}
