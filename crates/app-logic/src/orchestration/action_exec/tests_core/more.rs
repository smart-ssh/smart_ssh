use super::*;
use ssh_manager_core::sync::lock_tolerating_poison;

#[tokio::test]
async fn test_deny_path_executes_nothing_and_does_not_block_further_events() {
    let session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "curl evil.example".to_string(),
            }),
            AiEvent::TextDelta("weiterer Text nach der Ablehnung".to_string()),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    // Hard-Blacklist matcht "curl" nicht automatisch auf Deny (nur
    // Confirm, s. core::filter), daher hier eine explizite
    // Deny-Regel, um den reinen Deny-Pfad ohne Warten zu testen.
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
    let mut session = session;
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyCurlPolicyStore)));

    run_chat_turn(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
    )
    .await;

    let events = lock_tolerating_poison(&emitter.events).clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    // Deny blockiert nur die Ausführung, nicht den weiteren
    // Stream-Verlauf: das TextDelta danach kommt trotzdem an.
    assert_eq!(event_names, vec!["chat-action-proposed", "chat-text-delta"]);
    assert!(session
        .ai
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(
            &m.content,
            MessageContent::Text(t) if t.contains("weiterer Text")
        )));
    assert!(!session
        .ai
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(m.content, MessageContent::CommandResult { .. })));
    // Spec 0021, Abschnitt 3, Fall 4: der Blockier-Grund landet als
    // `ActionRejected` im Kontext, damit die KI (in der automatisch
    // ausgelösten Folgerunde) weiß, warum nichts ausgeführt wurde.
    assert!(session
        .ai
        .context
        .lock()
        .await
        .history
        .iter()
        .any(|m| matches!(
            &m.content,
            MessageContent::ActionRejected { command, reason: RejectionReason::Blocked(_) }
                if command == "curl evil.example"
        )));
}

/// Spec 0003 Abschnitt 5.2 / Spec 0007 Abschnitt 6, letzter Punkt:
/// `ProposeNoteUpdate` wartet **immer** auf Bestätigung, unabhängig von
/// der Filter-Engine — hier absichtlich mit `AllowEverythingPolicyStore`
/// (die für ein `SuggestCommand` sofort `AutoExec` ergäbe), um zu
/// zeigen, dass die Filter-Engine für diesen Aktionstyp gar nicht erst
/// gefragt wird.
#[tokio::test]
async fn test_propose_note_update_always_waits_for_confirmation_and_persists() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServer,
                new_content: "neuer Kontext".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    );
    // Spec 0016, Abschnitt 6: die KI liefert keine ID mehr — das Backend
    // löst `CurrentServer` selbst auf `session.server_id` auf.
    let expected_server_id = session.server_id;
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowEverythingPolicyStore)));
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
    let responder = async {
        loop {
            let action_id = {
                let events = lock_tolerating_poison(&emitter.events);
                events.iter().find_map(|(name, payload)| {
                    (name == "chat-action-proposed")
                        .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id) = action_id {
                let action_id: ActionId = action_id.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };

    tokio::join!(turn, responder);

    let events = lock_tolerating_poison(&emitter.events).clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec!["chat-action-proposed", "chat-action-result"]
    );
    let (_, proposed_payload) = &events[0];
    assert!(
        proposed_payload["decision"]["Confirm"].is_object(),
        "ProposeNoteUpdate muss immer Confirm sein, nie AutoExec"
    );

    let revisions = lock_tolerating_poison(&profile_store.note_revisions).clone();
    assert_eq!(revisions.len(), 1);
    assert_eq!(
        revisions[0].target,
        ssh_manager_core::profiles::NoteTarget::Server(expected_server_id),
        "CurrentServer muss auf session.server_id auflösen, nie auf eine von der KI \
         gelieferte ID (die KI kennt hier gar keine)"
    );
    assert!(session.ai.context.lock().await.history.iter().any(
        |m| matches!(&m.content, MessageContent::Text(t) if t.contains("<note_update_result"))
    ));
}

/// Kern des ADR-Vorschlags in diesem Modul-Doc: nach einem
/// tatsächlich ausgeführten Kommando bekommt die KI automatisch eine
/// Folgerunde, um dessen Ergebnis in eine Antwort zu fassen — vorher
/// endete `run_chat_turn` stattdessen wortlos nach dem
/// `chat-action-result`.
#[tokio::test]
async fn test_executed_action_triggers_automatic_followup_round_with_final_answer() {
    let session = session_with_ai_provider_allowing_everything(
        MockAiProvider::with_rounds(vec![
            vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "uptime".to_string(),
                }),
                AiEvent::Done,
            ],
            vec![
                AiEvent::TextDelta("Der Server läuft seit 3 Tagen.".to_string()),
                AiEvent::Done,
            ],
        ]),
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

    let events = lock_tolerating_poison(&emitter.events).clone();
    let event_names = event_names_excluding_auto_continuation(&events);
    assert_eq!(
        event_names,
        vec![
            "chat-action-proposed",
            "chat-action-result",
            "chat-text-delta"
        ]
    );

    let history = session.ai.context.lock().await.history.clone();
    assert!(history
        .iter()
        .any(|m| matches!(&m.content, MessageContent::CommandResult { .. })));
    assert!(history
        .iter()
        .any(|m| matches!(&m.content, MessageContent::Text(t) if t.contains("3 Tagen"))));
}

/// Aufgabenstellung Teil 1, Punkt 2/5 (Spec 0017, Abschnitt 2, letzter
/// Absatz): eine langsame KI-Antwort in einer Session darf einen
/// zeitnahen Befehl in einer anderen Session nicht ausbremsen. Session A
/// bekommt einen `AiProvider`, dessen `send()`-Stream erst antwortet, wenn
/// der Test es über `release` freigibt (ADR 0122, R8: ein Signal statt
/// einer festen Verzögerung — Session A bleibt so beliebig lange "mitten
/// im Turn", unabhängig davon, wie schnell die Maschine gerade ist)
/// — währenddessen muss `run_chat_turn` für Session B (über denselben
/// `SessionManager`, wie es zwei parallele `send_chat_message`-Aufrufe für
/// zwei offene Tabs täten) trotzdem fertig werden. Schlägt fehl, falls
/// `SessionManager` doch einen Lock über die gesamte Map hinweg über einen
/// Await-Punkt hält (die Regression, vor der Spec 0017 warnt) oder falls
/// `Session`s `context`/`transport`-Mutexe session-übergreifend geteilt
/// würden statt pro Session zu existieren — in beiden Fällen bliebe Session
/// B hängen, bis der Test (nach der 5s-Obergrenze) aufgibt, denn Session A
/// wird erst NACH Session B freigegeben.
struct SlowAiProvider {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl AiProvider for SlowAiProvider {
    fn send(
        &self,
        _context: RedactedContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        let entered = self.entered.clone();
        let release = self.release.clone();
        Box::pin(futures::stream::once(async move {
            entered.notify_one();
            release.notified().await;
            AiEvent::Done
        }))
    }
}

#[tokio::test]
async fn test_slow_session_does_not_block_concurrent_session_via_shared_manager() {
    let manager = SessionManager::new();
    let id_slow = Uuid::new_v4();
    let id_fast = Uuid::new_v4();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());

    manager.insert(
        id_slow,
        Arc::new(session_with_ai_provider(
            SlowAiProvider {
                entered: entered.clone(),
                release: release.clone(),
            },
            MockSshTransport::default(),
        )),
    );
    manager.insert(
        id_fast,
        Arc::new(test_session(
            vec![AiEvent::Done],
            MockSshTransport::default(),
        )),
    );

    let session_slow = manager.get(id_slow).unwrap();
    let session_fast = manager.get(id_fast).unwrap();
    let emitter_slow = TestEmitter::default();
    let emitter_fast = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let slow_turn = run_chat_turn(
        &session_slow,
        id_slow,
        &emitter_slow,
        &profile_store,
        &confirmations,
    );

    // `SessionManager::get` für Session B während Session A noch mitten
    // in ihrem (absichtlich unbegrenzt langsamen) Turn steckt — genau das,
    // was ein zweiter, gleichzeitiger `send_chat_message`-Aufruf für einen
    // anderen Tab täte. Erst NACHDEM Session B fertig ist, wird Session A
    // freigegeben — eine über Sessions hinweg gehaltene Sperre würde B
    // damit für immer blockieren, nicht nur messbar verzögern (ADR 0122,
    // R8: Reihenfolge/Signal statt enges Zeitfenster). Die 5s-Obergrenze
    // greift nur, wenn tatsächlich etwas hängt (Richtwert R6).
    let fast_turn = async {
        entered.notified().await;
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_chat_turn(
                &session_fast,
                id_fast,
                &emitter_fast,
                &profile_store,
                &confirmations,
            ),
        )
        .await
        .expect(
            "Session B wurde durch die langsame Session A blockiert — \
             SessionManager/Session-Locks sperren offenbar über Sessions hinweg",
        );
        release.notify_one();
    };

    tokio::join!(slow_turn, fast_turn);

    // Spec 0080, A2: Session B bekommt Runde 1 eines frischen Turns nur
    // mit `Done` — seit A2 (Klarstellung Q-BL-0259-01) also GENAU das
    // `chat-response-empty`-Event, nicht mehr gar keins. Diese Zeile
    // prüft nicht den Kern dieses Tests (Nebenläufigkeit, s. oben) —
    // sie hält nur fest, dass sich außer dieser einen, erwarteten
    // Ergänzung nichts an Session B geändert hat.
    let events_fast = lock_tolerating_poison(&emitter_fast.events).clone();
    let event_names: Vec<&str> = events_fast.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(event_names, vec!["chat-response-empty"]);
}
