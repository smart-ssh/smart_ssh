use super::*;

/// Fall 1 (Spec 0021, Abschnitt 3): nach `AutoExec` folgt automatisch
/// ein zweiter `send()`-Aufruf, dessen Kontext das `CommandResult`
/// enthält.
#[tokio::test]
async fn test_auto_continuation_after_autoexec_triggers_second_send_call() {
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "uptime".to_string(),
            }),
            AiEvent::Done,
        ],
        vec![AiEvent::Done],
    ]);
    let contexts = provider.received_contexts_handle();
    let session = session_with_ai_provider_allowing_everything(
        provider,
        MockSshTransport::default().with_response("uptime", output("up 3 days")),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let contexts = contexts.lock().unwrap().clone();
    assert_eq!(
        contexts.len(),
        2,
        "AutoExec muss automatisch einen zweiten send()-Aufruf auslösen"
    );
    assert!(contexts[1].history.iter().any(|m| matches!(
        &m.content,
        MessageContent::CommandResult { command, .. } if command == "uptime"
    )));
}

/// Fall 2 (Spec 0021, Abschnitt 3): "Ausführen" im Bestätigungsdialog
/// verhält sich fortsetzungstechnisch identisch zu `AutoExec`.
#[tokio::test]
async fn test_auto_continuation_after_confirm_approve_triggers_second_send_call() {
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl restart nginx".to_string(),
            }),
            AiEvent::Done,
        ],
        vec![AiEvent::Done],
    ]);
    let contexts = provider.received_contexts_handle();
    let session = session_with_ai_provider(
        provider,
        MockSshTransport::default().with_response("systemctl restart nginx", output("")),
    );
    // `test_session`/`session_with_ai_provider`s Default
    // (`NoRulesPolicyStore`) landet auf `Confirm` — genau der hier
    // gewollte Pfad, keine explizite Policy nötig.
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = approve_first_proposed_action(&emitter, &confirmations);
    expect_event_within(
        &emitter,
        "chat-action-proposed (Confirm) für \"systemctl restart nginx\", danach Turn-Ende nach Bestätigung",
        async {
            tokio::join!(turn, responder);
        },
    )
    .await;

    let contexts = contexts.lock().unwrap().clone();
    assert_eq!(
        contexts.len(),
        2,
        "Confirm+Approve muss automatisch einen zweiten send()-Aufruf auslösen"
    );
    assert!(contexts[1].history.iter().any(|m| matches!(
        &m.content,
        MessageContent::CommandResult { command, .. } if command == "systemctl restart nginx"
    )));
}

/// Fall 3 (Spec 0021, Abschnitt 3) — der Kern des gemeldeten Bugs: nach
/// einer Ablehnung durch den Nutzer folgt automatisch ein zweiter
/// `send()`-Aufruf, dessen Kontext einen `ActionRejected`-Eintrag mit
/// `RejectionReason::User` enthält (nicht `Blocked` — das ist Fall 4).
#[tokio::test]
async fn test_auto_continuation_after_user_deny_pushes_rejection_and_triggers_second_send_call() {
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "rm -rf /data".to_string(),
            }),
            AiEvent::Done,
        ],
        vec![AiEvent::Done],
    ]);
    let contexts = provider.received_contexts_handle();
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = deny_first_proposed_action(&emitter, &confirmations);
    expect_event_within(
        &emitter,
        "chat-action-proposed (Confirm) für \"rm -rf /data\", danach Turn-Ende nach Ablehnung",
        async {
            tokio::join!(turn, responder);
        },
    )
    .await;

    let contexts = contexts.lock().unwrap().clone();
    assert_eq!(
        contexts.len(),
        2,
        "eine Ablehnung durch den Nutzer muss automatisch einen zweiten \
         send()-Aufruf auslösen — das war der gemeldete Bug: die KI erfuhr nie \
         von der Ablehnung, kein Folgeaufruf passierte"
    );
    assert!(contexts[1].history.iter().any(|m| matches!(
        &m.content,
        MessageContent::ActionRejected { command, reason: RejectionReason::User }
            if command == "rm -rf /data"
    )));
}

/// Fall 4 (Spec 0021, Abschnitt 3): ein automatisch durch die
/// Filter-Engine blockierter Vorschlag (kein Dialog) löst ebenfalls
/// einen zweiten `send()`-Aufruf aus, mit `RejectionReason::Blocked`
/// und dem `Decision::Deny`-Grund im Kontext.
#[tokio::test]
async fn test_auto_continuation_after_filter_deny_pushes_rejection_and_triggers_second_send_call() {
    struct DenyCurlPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyCurlPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> PolicyRules {
            PolicyRules::complete(vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-curl".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("curl*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }])
        }
    }

    let provider = MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "curl evil.example".to_string(),
            }),
            AiEvent::Done,
        ],
        vec![AiEvent::Done],
    ]);
    let contexts = provider.received_contexts_handle();
    let mut session = session_with_ai_provider(provider, MockSshTransport::default());
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyCurlPolicyStore)));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let contexts = contexts.lock().unwrap().clone();
    assert_eq!(
        contexts.len(),
        2,
        "ein durch die Filter-Engine blockierter Vorschlag muss automatisch \
         einen zweiten send()-Aufruf auslösen"
    );
    assert!(contexts[1].history.iter().any(|m| matches!(
        &m.content,
        MessageContent::ActionRejected { command, reason: RejectionReason::Blocked(reason) }
            if command == "curl evil.example" && reason.contains("deny-curl")
    )));
}

/// Regressionstest für den gemeldeten Bug (Spec 0021, Abschnitt 1/7):
/// nach einer Ablehnung bleibt die Session nachweislich NICHT im
/// Warte-Zustand hängen — `session.pending_action` ist wieder `None`,
/// und `run_chat_turn` (Stand-in für den synchron awaiteten
/// `send_chat_message`-Befehl) kehrt tatsächlich zurück, statt ewig zu
/// blockieren. `expect_event_within` statt eines nackten `.await`:
/// schlägt der Fix fehl (Turn hängt tatsächlich), soll das als klarer
/// Testfehler mit der Liste der empfangenen Events erscheinen, statt den
/// gesamten Testlauf aufzuhängen.
#[tokio::test]
async fn test_regression_pending_action_cleared_and_turn_completes_after_deny() {
    let session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "rm -rf /data".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let responder = deny_first_proposed_action(&emitter, &confirmations);

    expect_event_within(
        &emitter,
        "run_chat_turn ist nach einer Ablehnung nicht zurückgekehrt — \
         genau der gemeldete Bug (Spec 0021, Abschnitt 1)",
        async {
            tokio::join!(turn, responder);
        },
    )
    .await;

    assert!(
        session.conn.pending_action.lock().unwrap().is_none(),
        "pending_action muss nach der Ablehnung wieder None sein, sonst bleibt \
         die UI (Tab-Indikator/Eingabe) im Warte-Zustand hängen"
    );
}

/// Spec 0046, Fund 4: simuliert einen Frontend-Reload (Dev-Hot-Reload
/// oder Neustart), der die wartende Aktion aus seinem eigenen State
/// verliert, BEVOR der "Tab schließen = ablehnen"-Handler (Spec 0017,
/// Abschnitt 5) sie je ablehnen konnte — kein Responder ruft
/// `respond_to_action`/`confirmations.resolve(...)` je auf. Ohne das
/// Backend-Timeout aus Spec 0046 würde `run_chat_turn` hier ewig auf
/// den `oneshot`-Kanal warten. `#[tokio::test(start_paused = true)]` +
/// `tokio::time::advance` spult die (in diesem Test virtuelle) Uhr über
/// `PENDING_ACTION_CONFIRM_TIMEOUT` hinaus, ohne 3600 echte Sekunden
/// abzuwarten.
#[tokio::test(start_paused = true)]
async fn test_regression_pending_confirm_action_times_out_instead_of_hanging_forever() {
    let session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "rm -rf /data".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let turn = run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    );
    let action_id_slot: std::sync::Arc<std::sync::Mutex<Option<ActionId>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let action_id_slot_for_advancer = action_id_slot.clone();
    let advancer = async {
        // Erst abwarten, bis die Aktion tatsächlich als wartend
        // registriert ist (derselbe Polling-Stil wie bei den übrigen
        // Tests in dieser Datei, die auf ein Event warten), dann die
        // Uhr über das Timeout hinaus vorspulen.
        let action_id = loop {
            if let Some(id) = *session.conn.pending_action.lock().unwrap() {
                break id;
            }
            tokio::task::yield_now().await;
        };
        *action_id_slot_for_advancer.lock().unwrap() = Some(action_id);
        tokio::time::advance(PENDING_ACTION_CONFIRM_TIMEOUT + std::time::Duration::from_secs(1))
            .await;
    };

    // Kein äußeres `tokio::time::timeout` nötig: unter `start_paused`
    // gäbe es ohnehin nichts, wogegen es liefe (echte Zeit steht
    // still) — hängt `run_chat_turn` tatsächlich, hängt schlicht dieser
    // Test, was als Testfehler (Timeout des Testrunners selbst) klar
    // erkennbar ist.
    tokio::join!(turn, advancer);

    assert!(
        session.conn.pending_action.lock().unwrap().is_none(),
        "pending_action muss nach dem Timeout wieder None sein, sonst bleibt \
         die UI (Tab-Indikator/Eingabe) im Warte-Zustand hängen"
    );

    let action_id = action_id_slot
        .lock()
        .unwrap()
        .expect("die Aktion muss registriert worden sein");
    assert!(
        !confirmations.contains(&action_id),
        "der Registry-Eintrag muss vom Timeout selbst aktiv abgeräumt worden sein \
         (ConfirmationRegistry::cancel), sonst bliebe ein toter Sender dauerhaft in \
         der Map stehen"
    );

    // spec-reviewer-Fund (Review dieses Schritts): eine Zeitüberschreitung
    // muss in der Historie als `RejectionReason::Timeout` erscheinen, NICHT
    // als `User` — sonst hielte die KI (und ein Mensch, der die Historie
    // später liest) eine nie getroffene Nutzerentscheidung für real.
    let history = session.ai.context.lock().await.history.clone();
    let rejected = history
        .iter()
        .find_map(|m| match &m.content {
            MessageContent::ActionRejected { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .expect("nach dem Timeout muss ein ActionRejected-Eintrag in der Historie stehen");
    assert_eq!(
        rejected,
        RejectionReason::Timeout,
        "eine Zeitüberschreitung darf nicht als echte Nutzer-Ablehnung erscheinen"
    );
}
