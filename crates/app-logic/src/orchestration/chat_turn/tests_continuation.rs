//! Tests für Auto-Titel-Generierung, automatische Turn-Fortsetzung nach
//! Aktionsergebnis, manuellen Stopp, Rate-Limit-/Pacing-Gates sowie
//! mehrere Tool-Calls in einer Antwort — Spec 0083: reine Verschiebung
//! aus `orchestration::tests`, keine Verhaltensänderung.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use async_trait::async_trait;
use uuid::Uuid;

use ssh_manager_core::ai::{AiProvider, RejectionReason, SessionContext};
use ssh_manager_core::filter::{Decision, EffectiveScope, FilterEngine, PolicyStore, Rule};
use ssh_manager_core::profiles::{CredentialStore, PostIngestPolicy};
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::mock::MockSftpSession;

use crate::dto::ActionUserDecision;
use crate::events::TestEmitter;
use crate::orchestration::generate_session_title_on_disconnect;
use crate::state::ActionId;

use super::super::test_support::*;
use super::*;

/// Spec 0034, Abschnitt 7: automatische Titel-Generierung — nur bei
/// mindestens einer Nutzer-Nachricht und nur, solange noch kein Titel
/// gesetzt ist.
#[tokio::test]
async fn test_auto_title_generation_sets_title_only_once() {
    let (session, chat_store, _chat_session_id, _tmp_dir) = session_with_real_chat_persistence(
        vec![
            AiEvent::TextDelta("Festplatte aufgeräumt".to_string()),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    )
    .await;
    push_history(
        &session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("räum mal die Festplatte auf".to_string()),
        },
    )
    .await;

    generate_session_title_on_disconnect(&session, Uuid::new_v4(), &TestEmitter::default()).await;

    let listed = chat_store
        .list_sessions_for_server(&session.server_id)
        .await
        .unwrap();
    assert_eq!(listed[0].title.as_deref(), Some("Festplatte aufgeräumt"));

    // Zweiter Aufruf (z. B. würde ein späteres erneutes `disconnect()`
    // nach einem Resume das auslösen) darf den bereits gesetzten Titel
    // NICHT überschreiben, obwohl der Mock-Provider bereitwillig
    // erneut antworten würde.
    generate_session_title_on_disconnect(&session, Uuid::new_v4(), &TestEmitter::default()).await;
    let listed_again = chat_store
        .list_sessions_for_server(&session.server_id)
        .await
        .unwrap();
    assert_eq!(
        listed_again[0].title.as_deref(),
        Some("Festplatte aufgeräumt")
    );
}

/// Gegenprobe: keine Nutzer-Nachricht in der Historie -> kein KI-Aufruf,
/// kein Titel.
#[tokio::test]
async fn test_auto_title_generation_skipped_without_user_message() {
    let (session, chat_store, _chat_session_id, _tmp_dir) = session_with_real_chat_persistence(
        vec![
            AiEvent::TextDelta("sollte nie ankommen".to_string()),
            AiEvent::Done,
        ],
        MockSshTransport::default(),
    )
    .await;

    generate_session_title_on_disconnect(&session, Uuid::new_v4(), &TestEmitter::default()).await;

    let listed = chat_store
        .list_sessions_for_server(&session.server_id)
        .await
        .unwrap();
    assert_eq!(listed[0].title, None);
}

// --- Spec 0021: Turn-Fortsetzung nach Aktionsergebnis -------------------

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
    let mut session = session_with_ai_provider(
        provider,
        MockSshTransport::default().with_response("uptime", output("up 3 days")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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
    tokio::join!(turn, responder);

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
    tokio::join!(turn, responder);

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
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-curl".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("curl*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
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
    session.filter_engine = Box::new(FilterEngine::new(DenyCurlPolicyStore));
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
/// blockieren. `tokio::time::timeout` statt eines nackten `.await`:
/// schlägt der Fix fehl (Turn hängt tatsächlich), soll das als klarer
/// Testfehler erscheinen, statt den gesamten Testlauf aufzuhängen.
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

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(turn, responder);
    })
    .await
    .expect(
        "run_chat_turn ist nach einer Ablehnung nicht zurückgekehrt — \
         genau der gemeldete Bug (Spec 0021, Abschnitt 1)",
    );

    assert!(
        session.pending_action.lock().unwrap().is_none(),
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
            if let Some(id) = *session.pending_action.lock().unwrap() {
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
        session.pending_action.lock().unwrap().is_none(),
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
    let history = session.context.lock().await.history.clone();
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

/// Spec 0021, Abschnitt 4: eine KI, die in jeder automatischen
/// Folgerunde erneut ein (durch die Filter-Engine blockiertes)
/// Kommando vorschlägt, läuft nicht endlos weiter, sondern hält nach
/// [`MAX_AUTO_FOLLOWUP_ROUNDS`] Runden mit einer sichtbaren
/// Chat-Systemnachricht an — anders als
/// `test_runaway_followup_rounds_are_bounded` (die dieselbe Grenze für
/// tatsächlich *ausgeführte* Aktionen prüft) zeigt dieser Test, dass
/// auch dauerhaft *blockierte* Vorschläge denselben Zähler verbrauchen.
#[tokio::test]
async fn test_auto_continuation_cap_stops_after_configured_rounds_with_visible_message() {
    struct AlwaysSuggestEchoProvider;
    impl AiProvider for AlwaysSuggestEchoProvider {
        fn send(
            &self,
            _context: SessionContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
            Box::pin(futures::stream::iter(vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "echo again".to_string(),
                }),
                AiEvent::Done,
            ]))
        }
    }
    struct DenyEchoPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyEchoPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-echo".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("echo*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }

    let mut session =
        session_with_ai_provider(AlwaysSuggestEchoProvider, MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(DenyEchoPolicyStore));
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

    let events = emitter.events.lock().unwrap().clone();
    let proposed_count = events
        .iter()
        .filter(|(name, _)| name == "chat-action-proposed")
        .count();
    assert_eq!(proposed_count, MAX_AUTO_FOLLOWUP_ROUNDS);
    // Spec 0021, Abschnitt 4 / ADR 0021: das Erreichen des Caps ist ein
    // weicher Stopp, kein Fehler — eigenes Event statt `chat-error`
    // (unabhängiger Review-Pass, Spec-Audit-Fund).
    let (last_name, last_payload) = events.last().unwrap();
    assert_eq!(last_name, "chat-auto-continuation-limit-reached");
    assert!(
        !events.iter().any(|(name, _)| name == "chat-error"),
        "Erreichen des Caps darf keinen chat-error auslösen: {events:?}"
    );
    assert_eq!(
        last_payload["limit"].as_u64().unwrap(),
        MAX_AUTO_FOLLOWUP_ROUNDS as u64,
    );

    let history = session.context.lock().await.history.clone();
    let rejected_count = history
        .iter()
        .filter(|m| matches!(m.content, MessageContent::ActionRejected { .. }))
        .count();
    assert_eq!(
        rejected_count, MAX_AUTO_FOLLOWUP_ROUNDS,
        "jede der {MAX_AUTO_FOLLOWUP_ROUNDS} Runden muss einen ActionRejected-Eintrag hinterlassen haben"
    );
}

/// Spec 0021, Abschnitt 4, letzter Satz: der Rundenzähler wird bei
/// jeder neuen Nutzer-Nachricht zurückgesetzt — hier simuliert durch
/// zwei aufeinanderfolgende `run_chat_turn`-Aufrufe auf derselben
/// Session (wie zwei aufeinanderfolgende `send_chat_message`-Befehle).
/// Beide Male muss die Automatik bis zum vollen Limit laufen dürfen,
/// nicht nur beim ersten Mal.
#[tokio::test]
async fn test_auto_continuation_cap_resets_for_each_new_user_message() {
    struct AlwaysSuggestEchoProvider;
    impl AiProvider for AlwaysSuggestEchoProvider {
        fn send(
            &self,
            _context: SessionContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
            Box::pin(futures::stream::iter(vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "echo again".to_string(),
                }),
                AiEvent::Done,
            ]))
        }
    }
    struct DenyEchoPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyEchoPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-echo".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("echo*".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }

    let mut session =
        session_with_ai_provider(AlwaysSuggestEchoProvider, MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(DenyEchoPolicyStore));
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let emitter1 = TestEmitter::default();
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter1,
        &profile_store,
        &confirmations,
    )
    .await;
    let first_proposed = emitter1
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "chat-action-proposed")
        .count();
    assert_eq!(first_proposed, MAX_AUTO_FOLLOWUP_ROUNDS);

    // Zweiter Aufruf = neue Nutzer-Nachricht.
    let emitter2 = TestEmitter::default();
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &emitter2,
        &profile_store,
        &confirmations,
    )
    .await;
    let second_proposed = emitter2
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "chat-action-proposed")
        .count();
    assert_eq!(
        second_proposed, MAX_AUTO_FOLLOWUP_ROUNDS,
        "der Rundenzähler darf nicht über die erste Nachricht hinaus fortbestehen"
    );
}

/// Spec 0021, Abschnitt 5: "Automatik stoppen" verhindert zuverlässig
/// weitere automatische Runden, lässt aber einen bereits offenen
/// Bestätigungsdialog unangetastet — hier simuliert durch direktes
/// Setzen von `session.auto_continue_stop`, während der Dialog der
/// zweiten (automatischen) Runde noch offen ist, genau der in
/// `app_shell::commands::stop_auto_continuation` gesetzte Zustand.
#[tokio::test]
async fn test_stop_auto_continuation_prevents_further_rounds_but_leaves_open_dialog_intact() {
    let provider = MockAiProvider::with_rounds(vec![
        // Runde 1: AutoExec (löst automatisch Runde 2 aus).
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "echo one".to_string(),
            }),
            AiEvent::Done,
        ],
        // Runde 2 (automatisch): `PostIngestPolicy::Strict` (s. u.)
        // stuft dieses SuggestCommand hoch, sobald in Runde 1
        // Serverinhalt eingelesen wurde — genau der hier gewollte
        // offene Dialog (Spec 0039, ersetzt die frühere SEC-03-
        // Rundenzähler-Bremse aus Spec 0013).
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "echo two".to_string(),
            }),
            AiEvent::Done,
        ],
        // Runde 3 darf durch den Stop NIE erreicht werden.
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "echo three".to_string(),
            }),
            AiEvent::Done,
        ],
    ]);
    let contexts = provider.received_contexts_handle();
    let mut session = session_with_ai_provider(
        provider,
        MockSshTransport::default()
            .with_response("echo one", output("one"))
            .with_response("echo two", output("two")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    // Spec 0039: der Dialog in Runde 2 muss verlässlich auftauchen,
    // damit "Automatik stoppen" mitten im offenen Dialog überhaupt
    // testbar ist — `Strict` eskaliert jede Aktion, sobald das Flag
    // (durch die Ausführung von "echo one" in Runde 1) gesetzt ist,
    // unabhängig davon, ob "echo two" selbst als "verändernd" gilt.
    session.post_ingest_policy = PostIngestPolicy::Strict;
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
            let confirm_action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    (name == "chat-action-proposed"
                        && payload
                            .get("decision")
                            .and_then(|d| d.get("Confirm"))
                            .is_some())
                    .then(|| payload["actionId"].as_str().unwrap().to_string())
                })
            };
            if let Some(action_id_str) = confirm_action_id {
                // "Automatik stoppen" WÄHREND der Dialog von Runde 2
                // noch offen ist — muss den Dialog selbst unangetastet
                // lassen (Spec 0021, Abschnitt 5, letzter Satz).
                session
                    .auto_continue_stop
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                let action_id: ActionId = action_id_str.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::join!(turn, responder);

    let contexts = contexts.lock().unwrap().clone();
    assert_eq!(
        contexts.len(),
        2,
        "Runde 2 (mit dem bereits offenen Dialog) muss noch laufen — nur \
         Runde 3 darf durch den Stop verhindert werden"
    );

    let history = session.context.lock().await.history.clone();
    assert!(
        history.iter().any(|m| matches!(
            &m.content,
            MessageContent::CommandResult { command, .. } if command == "echo two"
        )),
        "der bereits offene Dialog aus Runde 2 muss normal zu Ende laufen"
    );
    assert!(
        !history.iter().any(|m| matches!(
            &m.content,
            MessageContent::CommandResult { command, .. } if command == "echo three"
        )),
        "Runde 3 darf durch den Stop nie erreicht werden"
    );
    assert!(
        !emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|(name, _)| name == "chat-error"),
        "ein manueller Stop ist kein Fehlerfall, keine chat-error-Meldung erwartet"
    );
}

// --- Spec 0022: Credential-Caching (Sudo-Passwort) ----------------------

/// Spec 0022, Abschnitt 3, zweiter Punkt: das Sudo-Passwort wird laut
/// Spec 0018 einmalig bei `connect()` gelesen und in `Session.
/// sudo_password` gecacht — dieser Test verifiziert das über mehrere
/// tatsächlich ausgeführte `sudo`-Kommandos in derselben Session hinweg
/// (über die automatische Fortsetzung aus Spec 0021 erreicht, ohne dass
/// der Nutzer zwischendurch etwas eingeben muss), statt es nur an einer
/// einzelnen Ausführung zu prüfen.
#[tokio::test]
async fn test_sudo_password_credential_store_not_read_again_across_multiple_commands() {
    let credential_ref = crate::server_credentials::sudo_password_credential_ref(ServerId::new());
    let store =
        crate::test_support::InMemoryCredentialStore::new().with_secret(&credential_ref, "hunter2");

    // Exakt der Ablauf aus `app_shell::commands::connect` (Spec 0018,
    // Abschnitt 6): einmal lesen, danach in `Session.sudo_password`
    // cachen — kein Store-Zugriff mehr für den Rest der Session-Laufzeit.
    let resolved_password = store.get(&credential_ref).ok();
    assert_eq!(store.get_calls(), 1);

    let mut session = session_with_ai_provider(
        MockAiProvider::with_rounds(vec![
            // Runde 1: erstes sudo-Kommando — stuft schon wegen
            // `FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM` (Spec 0018,
            // unabhängiger Review-Fund) auf Confirm hoch, unabhängig von
            // der Runden-Nummer.
            vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "sudo systemctl restart nginx".to_string(),
                }),
                AiEvent::Done,
            ],
            // Runde 2 (automatisch, Spec 0021): die Sudo-Passwort-
            // Eskalation stuft auch dieses SuggestCommand auf Confirm
            // hoch (unabhängig von Runde/PostIngestPolicy, s. u.) —
            // zweites sudo-Kommando, über den Responder unten
            // bestätigt.
            vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: "sudo systemctl status nginx".to_string(),
                }),
                AiEvent::Done,
            ],
            vec![AiEvent::Done],
        ]),
        MockSshTransport::default()
            .with_response("sudo -S systemctl restart nginx", output(""))
            .with_response("sudo -S systemctl status nginx", output("active")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.sudo_password = resolved_password;

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
        // Beide Kommandos verlangen wegen der Sudo-Passwort-Eskalation
        // Confirm (unabhängig von Runde/PostIngestPolicy) — hier werden
        // beide der Reihe nach bestätigt, statt nur das erste gefundene.
        let mut resolved: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let confirm_action_id = {
                let events = emitter.events.lock().unwrap();
                events.iter().find_map(|(name, payload)| {
                    if name != "chat-action-proposed" {
                        return None;
                    }
                    payload.get("decision").and_then(|d| d.get("Confirm"))?;
                    let id = payload["actionId"].as_str().unwrap().to_string();
                    (!resolved.contains(&id)).then_some(id)
                })
            };
            if let Some(action_id_str) = confirm_action_id {
                resolved.insert(action_id_str.clone());
                let action_id: ActionId = action_id_str.parse().unwrap();
                confirmations
                    .resolve(&action_id, ActionUserDecision::Approve)
                    .unwrap();
                if resolved.len() == 2 {
                    break;
                }
            } else {
                tokio::task::yield_now().await;
            }
        }
    };
    tokio::join!(turn, responder);

    assert_eq!(
        store.get_calls(),
        1,
        "Sudo-Passwort darf nach dem Verbindungsaufbau nicht erneut aus dem \
         CredentialStore gelesen werden, auch nicht bei mehreren ausgeführten \
         sudo-Kommandos in derselben Session"
    );

    let history = session.context.lock().await.history.clone();
    let executed: Vec<&str> = history
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::CommandResult { command, .. } => Some(command.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        executed,
        vec![
            "sudo -S systemctl restart nginx",
            "sudo -S systemctl status nginx"
        ],
        "beide Kommandos müssen tatsächlich mit dem gecachten Passwort gelaufen sein"
    );
}

/// Spec 0051, Teil 2: zwei unmittelbar aufeinanderfolgende
/// `wait_for_ai_request_slot`-Aufrufe derselben Sitzung müssen um
/// mindestens `MIN_AI_REQUEST_SPACING` auseinanderliegen — das ist die
/// eigentliche "Burst-Entzerrung nachweisbar"-Prüfung aus der Spec.
/// `start_paused = true` lässt Tokios virtuelle Uhr bei einem
/// anstehenden `sleep` automatisch vorspringen, statt real zu warten
/// (s. Spec 0046, Fund 4 zur selben Technik in diesem Crate) — der Test
/// bleibt dadurch trotz der 300ms-Wartezeit sofort fertig.
#[tokio::test(start_paused = true)]
async fn test_wait_for_ai_request_slot_enforces_minimum_spacing_between_calls() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());

    wait_for_ai_request_slot(&session).await;
    let first_slot = tokio::time::Instant::now();
    wait_for_ai_request_slot(&session).await;
    let elapsed = first_slot.elapsed();

    assert!(
        elapsed >= MIN_AI_REQUEST_SPACING,
        "zweiter Slot kam nach {elapsed:?}, erwartet mindestens {MIN_AI_REQUEST_SPACING:?}"
    );
}

/// Gegenprobe zum Test oben: liegt der vorherige Aufruf bereits länger
/// als `MIN_AI_REQUEST_SPACING` zurück, wartet der nächste Aufruf gar
/// nicht mehr — die Entzerrung bremst nur echte Bursts, nicht jede
/// KI-Anfrage generell.
#[tokio::test(start_paused = true)]
async fn test_wait_for_ai_request_slot_does_not_wait_if_spacing_already_elapsed() {
    let session = test_session(vec![AiEvent::Done], MockSshTransport::default());

    wait_for_ai_request_slot(&session).await;
    tokio::time::advance(MIN_AI_REQUEST_SPACING * 2).await;
    let before_second_slot = tokio::time::Instant::now();
    wait_for_ai_request_slot(&session).await;
    let elapsed = before_second_slot.elapsed();

    assert!(
        elapsed < MIN_AI_REQUEST_SPACING,
        "durfte nicht erneut warten, tat es aber ({elapsed:?})"
    );
}

// --- Spec 0061: wait_for_rate_limit_budget --------------------------

fn low_budget_snapshot() -> ai_providers::RateLimitHeaderSnapshot {
    ai_providers::RateLimitHeaderSnapshot {
        input_tokens: ai_providers::RawCounter {
            limit: Some(1_000),
            remaining: Some(50), // 5% — klar unter der 15%-Schwelle
            reset_at: Some(std::time::Instant::now() + std::time::Duration::from_secs(30)),
        },
        ..Default::default()
    }
}

/// Spec 0061, Testbarkeit: "Budget unter 15% → Gate wartet bis Reset,
/// dann Send; UI-Event gefeuert."
#[tokio::test(start_paused = true)]
async fn test_wait_for_rate_limit_budget_waits_and_emits_event_when_budget_is_low() {
    let budget = ai_providers::ProviderBudgetGuard::new();
    budget.record_headers(low_budget_snapshot());
    let emitter = TestEmitter::default();
    let session_id = Uuid::new_v4();

    let before = tokio::time::Instant::now();
    wait_for_rate_limit_budget(&budget, 0, &emitter, session_id).await;
    let elapsed = before.elapsed();

    assert!(
        elapsed >= std::time::Duration::from_secs(29),
        "muss bis in die Nähe des Reset-Zeitpunkts warten, wartete nur {elapsed:?}"
    );
    let events = emitter.events.lock().unwrap();
    assert!(
        events.iter().any(|(name, _)| name == "ai-budget-waiting"),
        "muss das Warte-Event feuern, tatsächliche Events: {events:?}"
    );
}

/// Gegenprobe: reichlich Restbudget (weit über der 15%-Schwelle) darf
/// den Send nicht verzögern.
#[tokio::test(start_paused = true)]
async fn test_wait_for_rate_limit_budget_does_not_wait_when_budget_is_healthy() {
    let budget = ai_providers::ProviderBudgetGuard::new();
    budget.record_headers(ai_providers::RateLimitHeaderSnapshot {
        input_tokens: ai_providers::RawCounter {
            limit: Some(1_000),
            remaining: Some(900),
            reset_at: None,
        },
        ..Default::default()
    });
    let emitter = TestEmitter::default();

    let before = tokio::time::Instant::now();
    wait_for_rate_limit_budget(&budget, 10, &emitter, Uuid::new_v4()).await;

    assert!(
        before.elapsed() < std::time::Duration::from_millis(50),
        "gesundes Budget darf nicht warten lassen"
    );
    assert!(
        emitter.events.lock().unwrap().is_empty(),
        "kein Warte-Event, wenn gar nicht gewartet wurde"
    );
}

/// Spec 0061, Invariante: "ein header-loser Provider wird nie
/// blockiert" — ein frisch erzeugter Wächter, auf dem noch NIE
/// `record_headers` lief, darf den Send unter keinen Umständen
/// verzögern, egal wie groß die Schätzung ist.
#[tokio::test(start_paused = true)]
async fn test_wait_for_rate_limit_budget_never_blocks_a_header_less_provider() {
    let budget = ai_providers::ProviderBudgetGuard::new();
    let emitter = TestEmitter::default();

    let before = tokio::time::Instant::now();
    wait_for_rate_limit_budget(&budget, 999_999_999, &emitter, Uuid::new_v4()).await;

    assert!(before.elapsed() < std::time::Duration::from_millis(50));
    assert!(emitter.events.lock().unwrap().is_empty());
}

/// Spec 0061, Testbarkeit: "geschätzter Input > Rest-Input-TPM → Gate
/// wartet vorab" — unabhängig von der 15%-Schwelle: hier liegt das
/// Restbudget bei 90% (weit über der Schwelle), aber der geschätzte
/// Request ist größer als das verbleibende Kontingent.
#[tokio::test(start_paused = true)]
async fn test_wait_for_rate_limit_budget_waits_when_estimate_exceeds_remaining_even_above_threshold(
) {
    let budget = ai_providers::ProviderBudgetGuard::new();
    budget.record_headers(ai_providers::RateLimitHeaderSnapshot {
        input_tokens: ai_providers::RawCounter {
            limit: Some(1_000),
            remaining: Some(900),
            reset_at: Some(std::time::Instant::now() + std::time::Duration::from_secs(5)),
        },
        ..Default::default()
    });
    let emitter = TestEmitter::default();

    let before = tokio::time::Instant::now();
    wait_for_rate_limit_budget(&budget, 5_000, &emitter, Uuid::new_v4()).await;

    assert!(
        before.elapsed() >= std::time::Duration::from_secs(4),
        "ein Request, der das Restbudget übersteigt, muss vorab warten"
    );
}

/// Spec 0061, Invariante: "kein unbegrenztes Hängen" — fehlt der
/// Reset-Zeitpunkt, wird höchstens `MAX_PROACTIVE_WAIT` gewartet, nicht
/// ewig.
#[tokio::test(start_paused = true)]
async fn test_wait_for_rate_limit_budget_caps_wait_when_reset_is_missing() {
    let budget = ai_providers::ProviderBudgetGuard::new();
    budget.record_headers(ai_providers::RateLimitHeaderSnapshot {
        requests: ai_providers::RawCounter {
            limit: Some(10),
            remaining: Some(0),
            reset_at: None,
        },
        ..Default::default()
    });
    let emitter = TestEmitter::default();

    let before = tokio::time::Instant::now();
    wait_for_rate_limit_budget(&budget, 0, &emitter, Uuid::new_v4()).await;
    let elapsed = before.elapsed();

    assert!(
        elapsed <= std::time::Duration::from_secs(91),
        "darf nicht über die gedeckelte Maximalwartezeit hinaus hängen, wartete {elapsed:?}"
    );
}

/// Spec 0066, §1: Provider, dessen Stream erst ein Text-Delta liefert und
/// dann hängt, bis `gate` geöffnet wird — danach folgt `after_gate`.
/// `dropped` wird gesetzt, sobald der Stream verworfen ist (entspricht
/// dem Schließen der HTTP-Verbindung bei einem echten Provider).
struct GatedAiProvider {
    gate: Arc<tokio::sync::Notify>,
    after_gate: StdMutex<Option<Vec<AiEvent>>>,
    dropped: Arc<std::sync::atomic::AtomicBool>,
    send_calls: Arc<std::sync::atomic::AtomicUsize>,
}

struct DropFlag(Arc<std::sync::atomic::AtomicBool>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl AiProvider for GatedAiProvider {
    fn send(
        &self,
        _context: SessionContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        self.send_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let gate = self.gate.clone();
        let after_gate = self
            .after_gate
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| vec![AiEvent::Done]);
        let guard = DropFlag(self.dropped.clone());
        let head = futures::stream::iter(vec![AiEvent::TextDelta("Teil".to_string())]);
        let tail = futures::stream::once(async move {
            let _guard = guard;
            gate.notified().await;
            futures::stream::iter(after_gate)
        })
        .flatten();
        Box::pin(head.chain(tail))
    }
}

fn gated_provider(
    after_gate: Vec<AiEvent>,
) -> (
    GatedAiProvider,
    Arc<tokio::sync::Notify>,
    Arc<std::sync::atomic::AtomicBool>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let gate = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let send_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    (
        GatedAiProvider {
            gate: gate.clone(),
            after_gate: StdMutex::new(Some(after_gate)),
            dropped: dropped.clone(),
            send_calls: send_calls.clone(),
        },
        gate,
        dropped,
        send_calls,
    )
}

fn has_event(emitter: &TestEmitter, name: &str) -> bool {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|(event, _)| event == name)
}

/// Spec 0066, §1: Stopp während der Stream läuft bricht ihn sofort ab —
/// kein Warten auf das Ende der Antwort, Stream (= HTTP-Verbindung)
/// verworfen, bereits gestreamter Text bleibt im Verlauf.
#[tokio::test]
async fn test_stop_aborts_in_flight_ai_stream_immediately() {
    let (provider, _gate, dropped, send_calls) = gated_provider(vec![AiEvent::Done]);
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
    let stopper = async {
        while !has_event(&emitter, "chat-text-delta") {
            tokio::task::yield_now().await;
        }
        session.request_auto_continue_stop();
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(turn, stopper)
    })
    .await
    .expect("Stopp muss den hängenden Stream beenden, nicht auf sein Ende warten");

    assert!(
        dropped.load(std::sync::atomic::Ordering::SeqCst),
        "der Stream muss nach Stopp verworfen sein (schließt die Verbindung)"
    );
    assert_eq!(send_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(has_event(&emitter, "chat-response-cancelled"));
    let history = session.context.lock().await.history.clone();
    assert!(
        history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::Text(t) if t == "Teil")),
        "bereits gestreamter Text muss im Verlauf bleiben"
    );
}

/// Spec 0066, §1, Invariante: liegt nach einem Stopp bereits ein
/// vollständiger Tool-Call im Stream bereit, wird er trotzdem NIE an
/// Filter/Confirm/Ausführung weitergegeben.
#[tokio::test]
async fn test_stop_never_forwards_an_already_ready_tool_call() {
    let (provider, gate, _dropped, _send_calls) = gated_provider(vec![
        AiEvent::ActionProposed(AiAction::SuggestCommand {
            command: "echo gefaehrlich".to_string(),
        }),
        AiEvent::Done,
    ]);
    let mut session = session_with_ai_provider(
        provider,
        MockSshTransport::default().with_response("echo gefaehrlich", output("x")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
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
    let stopper = async {
        while !has_event(&emitter, "chat-text-delta") {
            tokio::task::yield_now().await;
        }
        // Beides gleichzeitig: Stopp setzen UND den Tool-Call sofort
        // verfügbar machen — der Stopp muss gewinnen.
        session.request_auto_continue_stop();
        gate.notify_waiters();
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(turn, stopper)
    })
    .await
    .expect("Turn muss nach Stopp enden");

    assert!(
        !has_event(&emitter, "chat-action-proposed"),
        "nach Stopp darf kein Tool-Call mehr vorgeschlagen werden"
    );
    let history = session.context.lock().await.history.clone();
    assert!(
        !history
            .iter()
            .any(|m| matches!(&m.content, MessageContent::CommandResult { .. })),
        "nach Stopp darf nichts ausgeführt werden"
    );
}

/// Spec 0066, §1: ein Stopp während der Wartezeit vor dem Send
/// (Pacing/Rate-Limit-Gate) verhindert den Request ganz.
#[tokio::test]
async fn test_stop_during_pre_send_wait_prevents_the_request() {
    let (provider, _gate, _dropped, send_calls) = gated_provider(vec![AiEvent::Done]);
    let session = session_with_ai_provider(provider, MockSshTransport::default());
    // Letzter Request "gerade eben" → `wait_for_ai_request_slot` schläft.
    *session.ai_request_paced_at.lock().await = Some(tokio::time::Instant::now());
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let session_id = Uuid::new_v4();

    let round = run_one_round(
        &session,
        session_id,
        &emitter,
        &profile_store,
        &confirmations,
        true,
    );
    let stopper = async {
        tokio::task::yield_now().await;
        session.request_auto_continue_stop();
    };
    let (outcome, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(round, stopper)
    })
    .await
    .expect("Stopp in der Wartezeit muss die Runde sofort beenden");

    assert_eq!(outcome, RoundOutcome::StoppedBeforeSend);
    assert_eq!(
        send_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nach Stopp in der Wartezeit darf kein Request mehr rausgehen"
    );
    assert!(has_event(&emitter, "chat-response-cancelled"));
}

/// Spec 0066, §2: eine während Runde 1 eingereihte Nachricht geht mit
/// der Anfrage von Runde 2 an die KI — als normale Nutzer-Nachricht,
/// nach dem Kommando-Ergebnis, nicht in Runde 1.
#[tokio::test]
async fn test_queued_message_is_sent_with_the_next_round() {
    let provider = MockAiProvider::with_rounds(vec![
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "echo one".to_string(),
            }),
            AiEvent::Done,
        ],
        vec![AiEvent::Done],
    ]);
    let contexts = provider.received_contexts_handle();
    let mut session = session_with_ai_provider(
        provider,
        MockSshTransport::default().with_response("echo one", output("one")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    // Entspricht einer Nachricht, die während Runde 1 ankam — entnommen
    // wird erst an der Grenze zu Runde 2.
    session
        .chat_turn
        .lock()
        .unwrap()
        .queued
        .push("bitte nur lesen".to_string());
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
    assert_eq!(contexts.len(), 2);
    let is_queued_text = |m: &ChatMessage| {
        m.role == Role::User
            && matches!(&m.content, MessageContent::Text(t) if t == "bitte nur lesen")
    };
    assert!(!contexts[0].history.iter().any(is_queued_text));
    let round_two = &contexts[1].history;
    let queued_at = round_two
        .iter()
        .position(is_queued_text)
        .expect("eingereihte Nachricht muss in Runde 2 mitgehen");
    let result_at = round_two
        .iter()
        .position(|m| matches!(&m.content, MessageContent::CommandResult { .. }))
        .unwrap();
    assert!(
        queued_at > result_at,
        "nach dem Kommando-Ergebnis einsortiert"
    );
    assert!(has_event(&emitter, "chat-queued-messages-sent"));
    assert!(session.chat_turn.lock().unwrap().queued.is_empty());
}

/// Spec 0066, spec-reviewer-Fund: ein Stopp, der eintrifft, nachdem ein
/// AutoExec-Vorschlag schon abgeholt ist, aber bevor die Ausführung
/// beginnt, verhindert die Ausführung — nur für den eigenen Chat.
#[tokio::test]
async fn test_stop_prevents_auto_exec_that_has_not_started_yet() {
    let mut session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default().with_response("echo nie", output("SOLLTE-NIE-LAUFEN")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.request_auto_continue_stop();
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let executed = handle_action_proposed(
        &session,
        Uuid::new_v4(),
        AiAction::SuggestCommand {
            command: "echo nie".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        ActionOrigin::Internal,
        test_fresh_rejection_flag(),
    )
    .await;

    assert!(!executed);
    let history = session.context.lock().await.history.clone();
    let result = history
        .iter()
        .find_map(|m| match &m.content {
            MessageContent::CommandResult {
                output, cancelled, ..
            } => Some((output.clone(), *cancelled)),
            _ => None,
        })
        .expect("die Karte braucht ein Ergebnis, sonst hängt sie auf „läuft“");
    assert!(result.1, "als abgebrochen gemeldet");
    assert!(
        !String::from_utf8_lossy(&result.0.stdout).contains("SOLLTE-NIE-LAUFEN"),
        "das Kommando darf nach Stopp nicht mehr gestartet werden"
    );
}

/// ADR 0058 §8: ein `sftp-server`-Aufruf wird trotz Allow-Regel nie
/// automatisch ausgeführt (Chat und MCP); `Deny` bleibt `Deny`.
#[tokio::test]
async fn test_sftp_server_invocation_always_requires_confirm_even_with_allow_rule() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    for origin in [
        ActionOrigin::Internal,
        ActionOrigin::Mcp { client_name: None },
    ] {
        let (decision, payload) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            proposed_decision_code_with_origin(
                &session,
                AiAction::SuggestCommand {
                    command: "sudo -n /usr/lib/openssh/sftp-server".to_string(),
                },
                origin,
            ),
        )
        .await
        .expect("Dialog muss enden");
        assert!(
            matches!(&decision, Decision::Confirm { code, .. }
                if code == "FILTER_SFTP_SERVER_REQUIRES_CONFIRM"),
            "{payload}"
        );
    }

    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "ls -l /usr/lib/openssh/sftp-server".to_string(),
            },
        ),
    )
    .await
    .expect("Aktion muss enden");
    assert!(matches!(decision, Decision::AutoExec), "{payload}");
}

/// Zweite Review-Runde (Spec 0068, ERHÖHT): eine Secret-Lese-Aktion darf
/// das Injection-Verdachts-Flag nicht verbrauchen — sonst liefe die
/// eigentliche Folgeaktion wieder automatisch. Der Dialog zeigt bei
/// gesetztem Flag den Injection-Grund.
#[tokio::test]
async fn test_secret_read_does_not_consume_injection_suspicion() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session
        .injection_suspected
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (first, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "cat .env".to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&first, Decision::Confirm { code, .. }
            if code == "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM"),
        "{payload}"
    );
    assert!(session
        .injection_suspected
        .load(std::sync::atomic::Ordering::SeqCst));

    let (second, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "systemctl restart nginx".to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&second, Decision::Confirm { code, .. }
            if code == "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM"),
        "Folgeaktion darf nicht automatisch laufen: {payload}"
    );
}

/// spec-reviewer-Fund (Spec 0068, ERHÖHT): derselbe Secret-Grund auch
/// für MCP-Herkunft — sichtbar im Dialog statt des allgemeinen
/// MCP-Grunds.
#[tokio::test]
async fn test_mcp_secret_path_read_shows_the_secret_reason() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code_with_origin(
            &session,
            AiAction::SuggestCommand {
                command: "cat /etc//shadow".to_string(),
            },
            ActionOrigin::Mcp { client_name: None },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

/// Spec 0068, Teil 2: Secret-Pfad-Lesen wird trotz Allow-Regel nie
/// automatisch ausgeführt — Chat-Kommando, `read_remote_file` und
/// MCP-Herkunft (s. auch den MCP-Test oben). Ein öffentlicher Schlüssel bleibt automatisch.
#[tokio::test]
async fn test_secret_path_read_always_requires_confirm_even_with_allow_rule() {
    let cases: Vec<(AiAction, bool)> = vec![
        (
            AiAction::SuggestCommand {
                command: "cat ~/.ssh/id_rsa".to_string(),
            },
            true,
        ),
        (
            AiAction::SuggestCommand {
                command: "echo ok; sudo tail /srv/app/.env".to_string(),
            },
            true,
        ),
        (
            AiAction::ReadRemoteFile {
                path: "/root/.ssh/id_ed25519".to_string(),
            },
            true,
        ),
        (
            AiAction::SuggestCommand {
                command: "cat ~/.ssh/id_rsa.pub".to_string(),
            },
            false,
        ),
        (
            AiAction::SuggestCommand {
                command: "cp ~/.ssh/id_rsa /backup/id_rsa".to_string(),
            },
            false,
        ),
    ];
    for (action, expect_confirm) in cases {
        let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
        session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();

        let handled = handle_action_proposed(
            &session,
            Uuid::new_v4(),
            action.clone(),
            &emitter,
            &profile_store,
            &confirmations,
            ActionOrigin::Internal,
            test_fresh_rejection_flag(),
        );
        let responder = async {
            // Ein offener Dialog wird abgelehnt, damit der Test endet.
            loop {
                let pending = emitter.events.lock().unwrap().iter().find_map(|(n, p)| {
                    (n == "chat-action-proposed" && p["decision"].get("Confirm").is_some())
                        .then(|| p["actionId"].as_str().unwrap().to_string())
                });
                if let Some(id) = pending {
                    let _ = confirmations.resolve(&id.parse().unwrap(), ActionUserDecision::Deny);
                    break;
                }
                if emitter
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(n, p)| n == "chat-action-proposed" && p["decision"] == "AutoExec")
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(handled, responder)
        })
        .await
        .expect("Aktion muss enden");

        let events = emitter.events.lock().unwrap().clone();
        let proposed = &events
            .iter()
            .find(|(n, _)| n == "chat-action-proposed")
            .expect("Vorschlag-Event")
            .1;
        let confirm = &proposed["decision"]["Confirm"];
        if expect_confirm {
            assert_eq!(
                confirm["code"], "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM",
                "{action:?}: {proposed}"
            );
        } else {
            assert!(
                confirm.is_null(),
                "{action:?} fälschlich eskaliert: {proposed}"
            );
        }
    }
}

/// Spec 0068, Teil 2: die Eskalation lockert nie — eine Deny-Regel
/// bleibt `Deny`, auch für einen Secret-Pfad.
#[tokio::test]
async fn test_secret_path_escalation_never_turns_deny_into_confirm() {
    struct DenyCatPolicyStore;
    #[async_trait]
    impl PolicyStore for DenyCatPolicyStore {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            vec![Rule {
                id: ssh_manager_core::filter::RuleId("deny-cat".to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob("cat *".to_string()),
                action: ssh_manager_core::filter::RuleAction::Deny,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            }]
        }
    }
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.filter_engine = Box::new(FilterEngine::new(DenyCatPolicyStore));

    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "cat ~/.ssh/id_rsa".to_string(),
            },
        ),
    )
    .await
    .expect("Deny darf keinen Dialog öffnen");

    assert!(matches!(decision, Decision::Deny { .. }), "{payload}");
}

/// Spec 0068, Teil 3 (Release-Gate C): ist ein Sudo-Passwort hinterlegt,
/// kündigt der Schreib-Dialog den möglichen Sudo-Fallback VOR der
/// Bestätigung an (`usesStoredSudoPassword`) — der Fallback nach einem
/// Rechte-Fehler passiert sonst still. Ohne hinterlegtes Passwort gibt
/// es keinen Fallback und keine Ankündigung.
#[tokio::test]
async fn test_write_confirmation_announces_possible_sudo_fallback() {
    let write = AiAction::WriteRemoteFile {
        path: "/etc/nginx/nginx.conf".to_string(),
        content: "worker_processes 2;".to_string(),
    };

    let mut with_password = test_session(vec![AiEvent::Done], MockSshTransport::default());
    with_password.sudo_password = Some(secrecy::SecretString::from("hunter2".to_string()));
    with_password
        .set_sftp_for_tests(Box::new(MockSftpSession::new()))
        .await;
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(&with_password, write.clone()),
    )
    .await
    .expect("Dialog muss enden");
    assert!(matches!(decision, Decision::Confirm { .. }), "{payload}");
    assert_eq!(payload["usesStoredSudoPassword"], true, "{payload}");

    let without = test_session(vec![AiEvent::Done], MockSshTransport::default());
    without
        .set_sftp_for_tests(Box::new(MockSftpSession::new()))
        .await;
    let (_, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(&without, write),
    )
    .await
    .expect("Dialog muss enden");
    assert_eq!(payload["usesStoredSudoPassword"], false, "{payload}");
}

/// Wie oben, über MCP (Review-Fund): derselbe Hinweis im Dialog.
#[tokio::test]
async fn test_mcp_write_confirmation_announces_possible_sudo_fallback() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.sudo_password = Some(secrecy::SecretString::from("hunter2".to_string()));
    session
        .set_sftp_for_tests(Box::new(MockSftpSession::new()))
        .await;
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code_with_origin(
            &session,
            AiAction::WriteRemoteFile {
                path: "/etc/nginx/nginx.conf".to_string(),
                content: "worker_processes 2;".to_string(),
            },
            ActionOrigin::Mcp { client_name: None },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(matches!(decision, Decision::Confirm { .. }), "{payload}");
    assert_eq!(payload["usesStoredSudoPassword"], true, "{payload}");
}

// --- Spec 0068, Teil 4: mehrere Tool-Calls in einer Antwort ----------

/// Allow-Regel nur für `ls*` — alles andere landet im Default-Confirm.
struct AllowLsOnly;
#[async_trait]
impl PolicyStore for AllowLsOnly {
    async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
        vec![Rule {
            id: ssh_manager_core::filter::RuleId("allow-ls".to_string()),
            pattern: ssh_manager_core::filter::Pattern::Glob("ls*".to_string()),
            action: ssh_manager_core::filter::RuleAction::Allow,
            scope: ssh_manager_core::filter::Scope::Global,
            priority: 0,
            origin: ssh_manager_core::filter::RuleOrigin::User,
        }]
    }
}

fn proposed_decisions(emitter: &TestEmitter) -> Vec<(String, serde_json::Value)> {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(n, _)| n == "chat-action-proposed")
        .map(|(_, p)| {
            (
                p["action"]["SuggestCommand"]["command"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                p["decision"].clone(),
            )
        })
        .collect()
}

fn executed_commands(session_history: &[ChatMessage]) -> Vec<String> {
    session_history
        .iter()
        .filter_map(|m| match &m.content {
            MessageContent::CommandResult { command, .. } => Some(command.clone()),
            _ => None,
        })
        .collect()
}

/// Beantwortet Dialoge der Reihe nach mit `decisions`; weitere Dialoge
/// bleiben unbeantwortet (dann greift das Test-Timeout).
async fn answer_dialogs_in_order(
    emitter: &TestEmitter,
    confirmations: &ConfirmationRegistry<ActionId, ActionUserDecision>,
    decisions: Vec<ActionUserDecision>,
) {
    let mut answered = std::collections::HashSet::new();
    let mut remaining = decisions.into_iter();
    loop {
        let pending = emitter.events.lock().unwrap().iter().find_map(|(n, p)| {
            let id = p["actionId"].as_str()?.to_string();
            (n == "chat-action-proposed"
                && p["decision"].get("Confirm").is_some()
                && !answered.contains(&id))
            .then_some(id)
        });
        if let Some(id) = pending {
            let Some(decision) = remaining.next() else {
                return;
            };
            confirmations
                .resolve(&id.parse().unwrap(), decision)
                .unwrap();
            answered.insert(id);
            continue;
        }
        tokio::task::yield_now().await;
        if remaining.len() == 0 {
            return;
        }
    }
}

#[tokio::test]
async fn test_two_tool_calls_get_their_own_filter_decision_each() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl restart nginx".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default()
            .with_response("ls -la", output("a"))
            .with_response("systemctl restart nginx", output("")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowLsOnly));
    session.post_ingest_policy = PostIngestPolicy::Standard;
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            run_chat_turn(
                &session,
                Uuid::new_v4(),
                &emitter,
                &profile_store,
                &confirmations,
            ),
            answer_dialogs_in_order(&emitter, &confirmations, vec![ActionUserDecision::Deny]),
        )
    })
    .await
    .expect("Turn muss enden");

    let decisions = proposed_decisions(&emitter);
    assert_eq!(decisions[0].0, "ls -la");
    assert_eq!(decisions[0].1, "AutoExec");
    assert_eq!(decisions[1].0, "systemctl restart nginx");
    assert!(decisions[1].1.get("Confirm").is_some(), "{decisions:?}");
    let history = session.context.lock().await.history.clone();
    assert_eq!(executed_commands(&history), vec!["ls -la".to_string()]);
}

/// Spec 0039 über Aktionsgrenzen: Aktion 1 liest Serverinhalt ein, Aktion
/// 2 DERSELBEN Antwort (verändernd) wird eskaliert.
#[tokio::test]
async fn test_untrusted_escalation_from_first_action_applies_to_second() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls /var/log".to_string(),
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl restart nginx".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default()
            .with_response("ls /var/log", output("syslog"))
            .with_response("systemctl restart nginx", output("")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    session.post_ingest_policy = PostIngestPolicy::Balanced;
    assert!(!session
        .untrusted_content_ingested
        .load(std::sync::atomic::Ordering::SeqCst));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            run_chat_turn(
                &session,
                Uuid::new_v4(),
                &emitter,
                &profile_store,
                &confirmations,
            ),
            answer_dialogs_in_order(&emitter, &confirmations, vec![ActionUserDecision::Deny]),
        )
    })
    .await
    .expect("Turn muss enden");

    let decisions = proposed_decisions(&emitter);
    assert_eq!(decisions[0].1, "AutoExec", "{decisions:?}");
    assert_eq!(
        decisions[1].1["Confirm"]["code"], "FILTER_POST_INGEST_REQUIRES_CONFIRM",
        "{decisions:?}"
    );
}

/// Ist-Verhalten dokumentiert (Spec 0068, Teil 4): lehnt der Nutzer
/// Aktion 1 ab, wird Aktion 2 trotzdem einzeln entschieden — weder still
/// mit abgelehnt noch still mit ausgeführt, sondern mit eigenem Dialog.
#[tokio::test]
async fn test_rejecting_first_action_leaves_second_to_its_own_decision() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl stop nginx".to_string(),
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl start nginx".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default()
            .with_response("systemctl stop nginx", output(""))
            .with_response("systemctl start nginx", output("")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowLsOnly));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            run_chat_turn(
                &session,
                Uuid::new_v4(),
                &emitter,
                &profile_store,
                &confirmations,
            ),
            answer_dialogs_in_order(
                &emitter,
                &confirmations,
                vec![ActionUserDecision::Deny, ActionUserDecision::Approve],
            ),
        )
    })
    .await
    .expect("Turn muss enden");

    let decisions = proposed_decisions(&emitter);
    assert!(decisions[0].1.get("Confirm").is_some());
    assert!(
        decisions[1].1.get("Confirm").is_some(),
        "eigener Dialog: {decisions:?}"
    );
    let history = session.context.lock().await.history.clone();
    assert_eq!(
        executed_commands(&history),
        vec!["systemctl start nginx".to_string()],
        "nur die einzeln bestätigte zweite Aktion läuft"
    );
}

/// Spec 0068, Teil 4 (Review-Fund): lehnt der Nutzer Aktion 1 ab, läuft
/// eine per Allow-Regel freigegebene Aktion 2 DERSELBEN Antwort nicht
/// mehr automatisch, sondern bekommt einen eigenen Dialog.
#[tokio::test]
async fn test_user_rejection_escalates_allowed_later_action_of_same_response() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl stop nginx".to_string(),
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default()
            .with_response("systemctl stop nginx", output(""))
            .with_response("ls -la", output("a")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowLsOnly));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            run_chat_turn(
                &session,
                Uuid::new_v4(),
                &emitter,
                &profile_store,
                &confirmations,
            ),
            answer_dialogs_in_order(
                &emitter,
                &confirmations,
                vec![ActionUserDecision::Deny, ActionUserDecision::Deny],
            ),
        )
    })
    .await
    .expect("Turn muss enden");

    let decisions = proposed_decisions(&emitter);
    assert_eq!(decisions[1].0, "ls -la");
    assert_eq!(
        decisions[1].1["Confirm"]["code"], "FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM",
        "{decisions:?}"
    );
    let history = session.context.lock().await.history.clone();
    assert!(executed_commands(&history).is_empty(), "{history:?}");
}

/// Zweite Review-Runde: bearbeitet der Nutzer Aktion 1 zu einem
/// Kommando, das eine Regel blockiert, zählt das ebenfalls als
/// Ablehnung — Aktion 2 läuft nicht automatisch.
#[tokio::test]
async fn test_blocked_edit_escalates_allowed_later_action_of_same_response() {
    struct DenyRmAllowLs2;
    #[async_trait]
    impl PolicyStore for DenyRmAllowLs2 {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            let rule = |id: &str, glob: &str, action| Rule {
                id: ssh_manager_core::filter::RuleId(id.to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob(glob.to_string()),
                action,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            };
            vec![
                rule(
                    "deny-rm",
                    "rm *",
                    ssh_manager_core::filter::RuleAction::Deny,
                ),
                rule(
                    "allow-ls",
                    "ls*",
                    ssh_manager_core::filter::RuleAction::Allow,
                ),
            ]
        }
    }
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl stop nginx".to_string(),
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("ls -la", output("a")),
    );
    session.filter_engine = Box::new(FilterEngine::new(DenyRmAllowLs2));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            run_chat_turn(
                &session,
                Uuid::new_v4(),
                &emitter,
                &profile_store,
                &confirmations,
            ),
            answer_dialogs_in_order(
                &emitter,
                &confirmations,
                vec![
                    ActionUserDecision::EditThenApprove {
                        command: "rm /tmp/x".to_string(),
                    },
                    ActionUserDecision::Deny,
                ],
            ),
        )
    })
    .await
    .expect("Turn muss enden");

    let decisions = proposed_decisions(&emitter);
    let ls = decisions
        .iter()
        .find(|(command, _)| command == "ls -la")
        .expect("ls wurde vorgeschlagen");
    assert_eq!(
        ls.1["Confirm"]["code"], "FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM",
        "{decisions:?}"
    );
}

/// Wie oben, aber Aktion 1 wird regelbasiert blockiert (`Deny`).
#[tokio::test]
async fn test_blocked_action_escalates_allowed_later_action_of_same_response() {
    struct DenyRmAllowLs;
    #[async_trait]
    impl PolicyStore for DenyRmAllowLs {
        async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
            let rule = |id: &str, glob: &str, action| Rule {
                id: ssh_manager_core::filter::RuleId(id.to_string()),
                pattern: ssh_manager_core::filter::Pattern::Glob(glob.to_string()),
                action,
                scope: ssh_manager_core::filter::Scope::Global,
                priority: 0,
                origin: ssh_manager_core::filter::RuleOrigin::User,
            };
            vec![
                rule(
                    "deny-rm",
                    "rm *",
                    ssh_manager_core::filter::RuleAction::Deny,
                ),
                rule(
                    "allow-ls",
                    "ls*",
                    ssh_manager_core::filter::RuleAction::Allow,
                ),
            ]
        }
    }
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "rm /tmp/x".to_string(),
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("ls -la", output("a")),
    );
    session.filter_engine = Box::new(FilterEngine::new(DenyRmAllowLs));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            run_chat_turn(
                &session,
                Uuid::new_v4(),
                &emitter,
                &profile_store,
                &confirmations,
            ),
            answer_dialogs_in_order(&emitter, &confirmations, vec![ActionUserDecision::Deny]),
        )
    })
    .await
    .expect("Turn muss enden");

    let decisions = proposed_decisions(&emitter);
    assert!(decisions[0].1.get("Deny").is_some(), "{decisions:?}");
    assert_eq!(
        decisions[1].1["Confirm"]["code"], "FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM",
        "{decisions:?}"
    );
}

/// Spec 0066 + 0068: Stopp, während Aktion 1 im Dialog steht — nach der
/// Bestätigung von Aktion 1 wird Aktion 2 nicht mehr vorgeschlagen und
/// nicht ausgeführt.
#[tokio::test]
async fn test_stop_between_two_tool_calls_prevents_the_second() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "systemctl reload nginx".to_string(),
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default()
            .with_response("systemctl reload nginx", output(""))
            .with_response("ls -la", output("a")),
    );
    session.filter_engine = Box::new(FilterEngine::new(AllowLsOnly));
    session.post_ingest_policy = PostIngestPolicy::Standard;
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    let responder = async {
        loop {
            let pending = emitter.events.lock().unwrap().iter().find_map(|(n, p)| {
                (n == "chat-action-proposed" && p["decision"].get("Confirm").is_some())
                    .then(|| p["actionId"].as_str().unwrap().to_string())
            });
            if let Some(id) = pending {
                session.request_auto_continue_stop();
                confirmations
                    .resolve(&id.parse().unwrap(), ActionUserDecision::Approve)
                    .unwrap();
                return;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            run_chat_turn(
                &session,
                Uuid::new_v4(),
                &emitter,
                &profile_store,
                &confirmations,
            ),
            responder,
        )
    })
    .await
    .expect("Turn muss enden");

    let decisions = proposed_decisions(&emitter);
    assert_eq!(
        decisions.len(),
        1,
        "Aktion 2 darf nach Stopp nicht kommen: {decisions:?}"
    );
    let history = session.context.lock().await.history.clone();
    assert_eq!(
        executed_commands(&history),
        vec!["systemctl reload nginx".to_string()]
    );
}
