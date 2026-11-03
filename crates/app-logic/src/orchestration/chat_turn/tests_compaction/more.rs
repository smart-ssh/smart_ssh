use super::*;

/// spec-reviewer-Fund (Review dieses Schritts): der Etappe-2-Test, der
/// die eigentliche Zusage der Leiter prüfte — "nach der Kompaktierung
/// passt der Request unters Budget" (Spec 0057, §3.2) — ging beim
/// Verschieben nach `orchestration::tests` (Etappe 3, `&Session`-
/// Parameter) verloren. Hier für BEIDE Pfade wiederhergestellt: mit
/// erfolgreicher Zusammenfassung (Schritt 1 allein reicht) und mit
/// fehlgeschlagener Zusammenfassung (Fallback auf den Etappe-2-
/// Platzhalter, der ebenfalls unters Budget passen muss — dafür ist
/// `determine_round_cut_count` überhaupt konservativ anhand der
/// Platzhalter-Größe bemessen, s. ADR 0049 Punkt 3).
#[tokio::test]
async fn test_compact_for_send_triggers_and_fits_under_budget_with_and_without_summary() {
    let history = vec![
        user_msg("Runde 1"),
        user_msg("Runde 2"),
        user_msg("Runde 3"),
        user_msg("Runde 4"),
        command_result_msg("cat big.log", &"a".repeat(100_000)),
    ];
    let budget = (10_000_f64 * 0.75) as usize;

    // Pfad 1: Zusammenfassung gelingt.
    {
        let session = session_with_ai_provider(
            MockAiProvider::new(vec![
                AiEvent::TextDelta("Kurze Zusammenfassung.".to_string()),
                AiEvent::Done,
            ]),
            MockSshTransport::default(),
        );
        {
            let mut ctx = session.ai.context.lock().await;
            ctx.history = history.clone();
        }
        let request_context = session.ai.context.lock().await.clone();
        let parts = session.ai.system_context_parts.lock().await.clone();
        let result = crate::compaction::compact_for_send(
            &session,
            Uuid::new_v4(),
            &TestEmitter::default(),
            request_context,
            &parts,
            10_000,
        )
        .await;
        let estimated = crate::compaction::estimate_request_tokens(&result);
        assert!(
            estimated <= budget,
            "mit erfolgreicher Zusammenfassung muss der Request unters Budget passen \
             (geschätzt {estimated}, Budget {budget})"
        );
    }

    // Pfad 2: Zusammenfassung schlägt fehl -> Fallback.
    {
        let session = session_with_ai_provider(
            MockAiProvider::new(vec![AiEvent::Error(AiError::RateLimited)]),
            MockSshTransport::default(),
        );
        {
            let mut ctx = session.ai.context.lock().await;
            ctx.history = history.clone();
        }
        let request_context = session.ai.context.lock().await.clone();
        let parts = session.ai.system_context_parts.lock().await.clone();
        let result = crate::compaction::compact_for_send(
            &session,
            Uuid::new_v4(),
            &TestEmitter::default(),
            request_context,
            &parts,
            10_000,
        )
        .await;
        let estimated = crate::compaction::estimate_request_tokens(&result);
        assert!(
            estimated <= budget,
            "auch der Fallback-Platzhalter (ohne Zusammenfassung) muss unters Budget \
             passen (geschätzt {estimated}, Budget {budget})"
        );
    }
}

/// Spec 0057, §2.1: greift Schritt 1 der Kürzungs-Leiter, wird beim
/// ERSTEN Mal eine echte Zusammenfassung erzeugt (kein bisheriger
/// Stand vorhanden, der wiederverwendet werden könnte) — der
/// Platzhalter muss den KI-generierten Text tragen, nicht den
/// generischen Etappe-2-Hinweis, und `session.summary` muss
/// aktualisiert sein.
#[tokio::test]
async fn test_compact_for_send_uses_summary_when_the_call_succeeds() {
    let mut session = session_with_ai_provider(
        MockAiProvider::new(vec![
            AiEvent::TextDelta("Nutzer prüfte Logs, alles unauffällig.".to_string()),
            AiEvent::Done,
        ]),
        MockSshTransport::default(),
    );
    session.set_model_context_window_tokens_for_tests(2_000);
    push_synthetic_rounds(&session, 6, 5_000).await;

    let request_context = session.ai.context.lock().await.clone();
    let parts = session.ai.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.ai.model_context_window_tokens,
    )
    .await;

    let placeholder_text = result
        .history
        .iter()
        .find_map(|m| match &m.content {
            MessageContent::Text(t) if t.contains(summary_marker(parts.language)) => {
                Some(t.clone())
            }
            _ => None,
        })
        .expect("ein Zusammenfassungs-Platzhalter muss in der gekürzten Historie stehen");
    assert!(
        placeholder_text.contains("Nutzer prüfte Logs, alles unauffällig."),
        "der Platzhalter muss den tatsächlichen KI-Text tragen: {placeholder_text}"
    );
    assert!(
        !placeholder_text.contains(truncation_marker(parts.language)),
        "bei Erfolg darf NICHT der generische Etappe-2-Hinweis verwendet werden"
    );
    let stored = session.ai.summary.lock().await.clone();
    assert_eq!(
        stored.map(|s| s.text),
        Some("Nutzer prüfte Logs, alles unauffällig.".to_string())
    );
}

/// Spec 0057, §2.1: "nicht jedes Mal die ganze History von vorne" —
/// deckt eine bereits gespeicherte Zusammenfassung den benötigten
/// `cut_count` schon ab, darf KEIN zweiter KI-Aufruf stattfinden.
#[tokio::test]
async fn test_compact_for_send_reuses_existing_summary_without_a_new_ai_call() {
    let received_contexts = std::sync::Arc::new(StdMutex::new(Vec::new()));
    let mut session = session_with_ai_provider(
        MockAiProvider {
            rounds: StdMutex::new(vec![vec![AiEvent::Done]].into()),
            received_contexts: received_contexts.clone(),
        },
        MockSshTransport::default(),
    );
    session.set_model_context_window_tokens_for_tests(2_000);
    push_synthetic_rounds(&session, 6, 5_000).await;
    *session.ai.summary.lock().await = Some(crate::compaction::RollingSummary {
        text: "Bereits vorhandene Zusammenfassung.".to_string(),
        // Großzügig: deckt mehr Runden ab, als für das aktuelle Budget
        // überhaupt gekürzt werden müssten.
        rounds_covered: 6,
    });

    let request_context = session.ai.context.lock().await.clone();
    let parts = session.ai.system_context_parts.lock().await.clone();
    let result = crate::compaction::compact_for_send(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        request_context,
        &parts,
        session.ai.model_context_window_tokens,
    )
    .await;

    assert!(
        received_contexts.lock().unwrap().is_empty(),
        "die vorhandene Summary deckt den Bedarf bereits ab — es darf kein KI-Aufruf \
         stattfinden"
    );
    assert!(
        result
            .history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("Bereits vorhandene Zusammenfassung."))),
        "die wiederverwendete Summary muss im gesendeten Kontext stehen"
    );
}
