use super::*;

#[tokio::test(start_paused = true)]
async fn test_issue_107_disconnect_suggestion_timeout_leaves_no_entry_and_rejects_late_resolve() {
    let session = disconnect_session_with_note_proposal();
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let flow = suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    );
    let advancer = async {
        let action_id = await_registered_suggestion(&emitter, &confirmations).await;
        tokio::time::advance(PENDING_ACTION_CONFIRM_TIMEOUT + std::time::Duration::from_secs(1))
            .await;
        action_id
    };
    let (suggested, action_id) = tokio::join!(flow, advancer);

    assert!(suggested, "der Vorschlag wurde emittiert");
    assert!(!confirmations.contains(&action_id));
    assert!(
        confirmations
            .resolve(&action_id, ActionUserDecision::Approve)
            .is_err(),
        "nach dem Timeout muss ein spätes `resolve` fehlschlagen"
    );
    assert!(
        profile_store.note_revisions.lock().unwrap().is_empty(),
        "ein Timeout gilt als Ablehnung"
    );
    assert_expired_event_emitted(&emitter, action_id);
}

/// Issue #628: the timeout tells the frontend which card expired.
fn assert_expired_event_emitted(emitter: &TestEmitter, action_id: ActionId) {
    let events = emitter.events.lock().unwrap();
    let expired: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "note-update-expired")
        .collect();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].1["actionId"], action_id.to_string());
}

#[tokio::test(start_paused = true)]
async fn test_issue_107_note_shrink_timeout_leaves_no_entry_and_rejects_late_resolve() {
    let provider = shrink_provider();
    let redactor = DefaultOutputRedactor::new();
    let server_id = ServerId::new();
    let profile_store = crate::test_support::InMemoryProfileStore::new()
        .with_server(server_with_notes(server_id, "Unveränderte Notiz."));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let target = shrink_target(&profile_store, server_id);
    let budget = ai_providers::ProviderBudgetGuard::new();

    let flow = execute_note_shrink_request(
        Uuid::new_v4(),
        server_id,
        &provider,
        &budget,
        &redactor,
        &emitter,
        &target,
        &confirmations,
        PromptLanguage::De,
    );
    let advancer = async {
        let action_id = await_registered_suggestion(&emitter, &confirmations).await;
        tokio::time::advance(PENDING_ACTION_CONFIRM_TIMEOUT + std::time::Duration::from_secs(1))
            .await;
        action_id
    };
    let ((), action_id) = tokio::join!(flow, advancer);

    assert!(!confirmations.contains(&action_id));
    assert!(confirmations
        .resolve(&action_id, ActionUserDecision::Approve)
        .is_err());
    assert_eq!(
        profile_store.get_server(&server_id).await.unwrap().notes,
        "Unveränderte Notiz."
    );
    assert_expired_event_emitted(&emitter, action_id);
}

/// Der Vorschlag beim Verbindungsende ist app-weit (Spec 0010, Abschnitt
/// 2, Punkt 6) — der Tab-Indikator darf weder während des Wartens noch
/// danach gesetzt sein. Geprüft wird in dem Moment, in dem der Ablauf
/// nachweislich auf die Entscheidung wartet.
///
/// Die Notiz-Kürzung bekommt keine `Session` übergeben und kann den
/// Indikator deshalb gar nicht setzen; ihr Guard
/// (`RegisteredConfirmation`) hält ebenfalls keine.
#[tokio::test]
async fn test_issue_107_disconnect_suggestion_never_sets_the_pending_action_indicator() {
    let session = disconnect_session_with_note_proposal();
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let flow = suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = async {
        let action_id = await_registered_suggestion(&emitter, &confirmations).await;
        // Ein paar weitere Runden, damit der Ablauf sicher im Warten steckt.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            *session.conn.pending_action.lock().unwrap(),
            None,
            "eine app-weite Notiz-Benachrichtigung darf keinen Tab-Indikator setzen"
        );
        confirmations
            .resolve(&action_id, ActionUserDecision::Deny)
            .unwrap();
    };
    let (suggested, ()) = tokio::time::timeout(ISSUE_107_TEST_TIMEOUT, async {
        tokio::join!(flow, responder)
    })
    .await
    .expect("der Ablauf muss nach der Ablehnung enden");

    assert!(suggested);
    assert_eq!(*session.conn.pending_action.lock().unwrap(), None);
}

/// Issue #330: Titel- und Abschluss-Instruktion folgen der Prompt-Sprache.
#[test]
fn test_title_and_disconnect_instructions_follow_the_prompt_language() {
    use crate::system_prompt::{prompt_language, PromptLanguage};
    let lang = |c: &str| prompt_language(Some(&serde_json::json!(c)), None);
    let de_title = super::title_generation_instruction(PromptLanguage::De);
    assert!(!de_title.contains("beendet"));
    assert!(de_title.starts_with("Fasse den Zweck"));
    let en_title = super::title_generation_instruction(PromptLanguage::En);
    assert!(!en_title.contains("ending now"));
    assert!(en_title.starts_with("Summarize the purpose"));
    assert!(!en_title.contains("Always respond in"));
    assert_eq!(
        super::title_generation_instruction(lang("fr")),
        format!("{en_title} Always respond in French.")
    );
    assert_eq!(super::title_generation_instruction(lang("xx")), en_title);

    let de = super::disconnect_completion_instruction(PromptLanguage::De);
    assert!(de.contains("künftige Sitzungen"));
    let en = super::disconnect_completion_instruction(PromptLanguage::En);
    assert!(en.starts_with("The session is ending now"));
    assert_eq!(
        super::disconnect_completion_instruction(lang("fr")),
        format!("{en} Always respond in French.")
    );
    assert_eq!(super::disconnect_completion_instruction(lang("xx")), en);
}

/// Issue #359: Abschluss- und Kürzen-Instruktion beschreiben in beiden
/// Sprachen dasselbe Notiz-Format und folgen der Prompt-Sprache.
#[test]
fn test_disconnect_and_shrink_instructions_carry_the_note_format_in_both_languages() {
    use crate::system_prompt::{NOTE_FORMAT_DE, NOTE_FORMAT_EN};
    let d_de = super::disconnect_completion_instruction(PromptLanguage::De);
    let d_en = super::disconnect_completion_instruction(PromptLanguage::En);
    let s_de = super::note_shrink_instruction(PromptLanguage::De);
    let s_en = super::note_shrink_instruction(PromptLanguage::En);
    for text in [&d_de, &s_de] {
        assert!(text.contains(NOTE_FORMAT_DE));
        assert!(text.contains("7.500 Zeichen"));
        assert!(!text.contains("getroffene Entscheidungen"));
    }
    for text in [&d_en, &s_en] {
        assert!(text.contains(NOTE_FORMAT_EN));
        assert!(text.contains("7,500 characters"));
        assert!(text.contains("rewrite it into this format"));
        assert!(!text.contains("decisions made"));
    }
    assert!(s_en.starts_with("Rewrite the following"));
    assert!(s_de.starts_with("Schreibe die folgende"));
    assert!(
        super::note_shrink_instruction(crate::system_prompt::prompt_language(
            Some(&serde_json::json!("fr")),
            None
        ))
        .ends_with("Always respond in French.")
    );
}

/// Issue #359: die Kürzen-Anfrage nutzt die englische Instruktion für
/// Englisch.
#[tokio::test]
async fn test_summarize_note_for_shrink_uses_the_prompt_language_instruction() {
    let provider = MockAiProvider::new(vec![AiEvent::TextDelta("x".to_string()), AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let redactor = DefaultOutputRedactor::new();
    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();
    summarize_note_for_shrink(
        &provider,
        &budget,
        &emitter,
        Uuid::new_v4(),
        &redactor,
        "srv",
        "note",
        PromptLanguage::En,
    )
    .await;
    let sent = contexts.lock().unwrap();
    let MessageContent::Text(text) = &sent[0].history[0].content else {
        panic!("expected text");
    };
    assert!(text.starts_with("Rewrite the following"), "{text}");
}

// --- Issue #564: disconnect note-update request goes through sealing ---

/// Extension whose content cannot be resolved, so sealing fails.
struct GoneExt;

#[async_trait::async_trait]
impl crate::chat_content_extension::ChatContentExtension for GoneExt {
    fn name(&self) -> &str {
        "files"
    }
    fn take_pending(
        &self,
        _s: Uuid,
        _id: &str,
    ) -> Option<crate::chat_content_extension::PendingContent> {
        None
    }
    async fn persist(
        &self,
        _s: Uuid,
        _part_id: &str,
        _b: Vec<ssh_manager_core::ai::RedactedBlock>,
    ) -> Result<(), crate::chat_content_extension::ChatContentError> {
        Ok(())
    }
    async fn resolve(
        &self,
        _s: Uuid,
        _part: &ssh_manager_core::ai::ExtensionPart,
    ) -> Result<
        Option<Vec<ssh_manager_core::ai::ResolvedBlock>>,
        crate::chat_content_extension::ChatContentError,
    > {
        Err(crate::chat_content_extension::ChatContentError(
            "gone".into(),
        ))
    }
}

/// Issue #564: when sealing fails, no provider request is made, no note
/// update is proposed and the function returns `false`.
#[tokio::test]
async fn test_issue_564_disconnect_suggestion_not_sent_when_sealing_fails() {
    let provider = MockAiProvider::new(vec![note_proposal("Neue Notiz"), AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let mut session = session_with_ai_provider(provider, MockSshTransport::default());
    session.set_chat_content_for_tests(std::sync::Arc::new(
        crate::chat_content_extension::ChatContentRegistry::new(vec![std::sync::Arc::new(GoneExt)]),
    ));
    {
        let mut ctx = session.ai.context.lock().await;
        ctx.history.push(command_result_message());
        ctx.history.push(ChatMessage {
            role: Role::User,
            content: MessageContent::Extension(ssh_manager_core::ai::ExtensionPart {
                extension: "files".into(),
                kind: "doc".into(),
                id: Uuid::new_v4().to_string(),
                label: "a.txt".into(),
                media: ssh_manager_core::ai::ExtensionMedia::Text,
                estimated_tokens: 1,
            }),
        });
    }
    let emitter = TestEmitter::default();
    let suggested = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        suggest_note_update_on_disconnect(
            &session,
            Uuid::new_v4(),
            &emitter,
            &InMemoryProfileStore::default(),
            &ConfirmationRegistry::new(),
        ),
    )
    .await
    .expect("disconnect must complete");
    assert!(contexts.lock().unwrap().is_empty(), "no provider request");
    assert!(emitter.events.lock().unwrap().is_empty(), "no proposal");
    assert!(!suggested);
}

/// Issue #564: for a plain session the request is unchanged (history plus
/// the completion instruction) and a planted secret is absent from it.
#[tokio::test]
async fn test_issue_564_disconnect_request_plain_session_and_secret_absent() {
    let provider = MockAiProvider::new(vec![AiEvent::Done]);
    let contexts = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    {
        let mut ctx = session.ai.context.lock().await;
        ctx.history.push(ChatMessage {
            role: Role::User,
            content: MessageContent::Text(format!("prüfe password={SECRET_0096} bitte")),
        });
        ctx.history.push(command_result_message());
    }
    suggest_note_update_on_disconnect(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;
    let sent = contexts.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    let history = &sent[0].history;
    assert_eq!(history.len(), 3, "user, command result, instruction");
    assert!(matches!(
        history[1].content,
        MessageContent::CommandResult { .. }
    ));
    let MessageContent::Text(first) = &history[0].content else {
        panic!("plain text expected");
    };
    assert!(first.starts_with("prüfe"));
    assert!(!format!("{history:?}").contains(SECRET_0096));
}
