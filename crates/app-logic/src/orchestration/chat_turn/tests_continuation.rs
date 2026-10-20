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
use ssh_manager_core::profiles::PostIngestPolicy;
use ssh_manager_core::shared::ServerId;
use ssh_manager_core::ssh::mock::MockSftpSession;

use crate::dto::ActionUserDecision;
use crate::events::TestEmitter;
use crate::orchestration::generate_session_title_on_disconnect;
use crate::state::ActionId;

use super::super::test_support::*;
use super::test_events::*;
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

// --- Issue #551: Titel nach der ersten Antwort, Ersatztitel ----------------

async fn title_of(
    chat_store: &persistence_sqlite::SqliteChatSessionStore,
    session: &Session,
) -> Option<String> {
    chat_store
        .list_sessions_for_server(&session.server_id)
        .await
        .unwrap()[0]
        .title
        .clone()
}

async fn run_first_turn(session: &Session, user_text: &str) {
    push_history(
        session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text(user_text.to_string()),
        },
    )
    .await;
    run_chat_turn(
        session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;
}

/// Issue #551: der Titel steht nach der ersten fertigen Antwort, ohne
/// Trennen — und wird nur einmal angefragt.
#[tokio::test]
async fn test_issue_551_title_is_generated_after_first_answer_once() {
    let (mut session, chat_store, _id, _tmp) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    let provider = MockAiProvider::with_rounds(vec![
        vec![AiEvent::TextDelta("Antwort".to_string()), AiEvent::Done],
        vec![
            AiEvent::TextDelta("Plattenplatz prüfen".to_string()),
            AiEvent::Done,
        ],
        vec![AiEvent::TextDelta("Antwort 2".to_string()), AiEvent::Done],
        vec![
            AiEvent::TextDelta("Anderer Titel".to_string()),
            AiEvent::Done,
        ],
    ]);
    let contexts = provider.received_contexts_handle();
    session.set_ai_provider_for_tests(Box::new(provider));

    run_first_turn(&session, "wie voll ist die Platte?").await;
    assert_eq!(
        title_of(&chat_store, &session).await.as_deref(),
        Some("Plattenplatz prüfen")
    );
    assert_eq!(contexts.lock().unwrap().len(), 2, "Antwort + Titel");

    run_first_turn(&session, "und jetzt?").await;
    assert_eq!(
        title_of(&chat_store, &session).await.as_deref(),
        Some("Plattenplatz prüfen")
    );
    assert_eq!(
        contexts.lock().unwrap().len(),
        3,
        "spätere Turns fragen keinen neuen Titel an"
    );
}

/// Issue #551: Fehler / abgeschnitten / leer / nur Platzhalter -> Ersatztitel
/// aus der geschwärzten, zusammengezogenen, gekürzten ersten Nachricht.
#[tokio::test]
async fn test_issue_551_failed_ai_title_falls_back_to_first_user_message() {
    let failures: Vec<Vec<AiEvent>> = vec![
        vec![AiEvent::Error(ssh_manager_core::ai::AiError::RateLimited)],
        vec![
            AiEvent::TextDelta("halb".to_string()),
            AiEvent::TextTruncated,
        ],
        vec![AiEvent::Done],
        vec![
            AiEvent::TextDelta("password=Geheim-0096".to_string()),
            AiEvent::Done,
        ],
    ];
    for failure in failures {
        let (mut session, chat_store, _id, _tmp) =
            session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default())
                .await;
        session.set_ai_provider_for_tests(Box::new(MockAiProvider::with_rounds(vec![
            vec![AiEvent::TextDelta("Antwort".to_string()), AiEvent::Done],
            failure,
        ])));
        run_first_turn(&session, "  bitte\n  die   Platte\tprüfen ").await;
        assert_eq!(
            title_of(&chat_store, &session).await.as_deref(),
            Some("bitte die Platte prüfen")
        );
    }
}

/// Issue #551: ein Geheimnis, das über die 60-Zeichen-Grenze reicht, landet
/// nicht im Ersatztitel (erst schwärzen, dann kürzen).
#[tokio::test]
async fn test_issue_551_fallback_title_redacts_before_truncating() {
    let (mut session, chat_store, _id, _tmp) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    session.set_ai_provider_for_tests(Box::new(MockAiProvider::with_rounds(vec![
        vec![AiEvent::TextDelta("Antwort".to_string()), AiEvent::Done],
        vec![AiEvent::Done],
    ])));
    let filler = "a".repeat(45);
    run_first_turn(
        &session,
        &format!("{filler} password={SECRET_0096} bitte prüfen"),
    )
    .await;
    let title = title_of(&chat_store, &session).await.expect("Ersatztitel");
    assert_no_fragment_of_secret(&title);
    assert!(title.chars().count() <= 60);
    assert!(title.starts_with(&filler));

    // nur Platzhalter -> bleibt ohne Titel
    let (mut session, chat_store, _id, _tmp) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    session.set_ai_provider_for_tests(Box::new(MockAiProvider::with_rounds(vec![
        vec![AiEvent::TextDelta("Antwort".to_string()), AiEvent::Done],
        vec![AiEvent::Done],
    ])));
    run_first_turn(&session, &format!("password={SECRET_0096}")).await;
    assert_eq!(title_of(&chat_store, &session).await, None);
}

/// Issue #551: ein vorher manuell vergebener Titel bleibt.
#[tokio::test]
async fn test_issue_551_manual_title_wins() {
    let (mut session, chat_store, id, _tmp) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    session.set_ai_provider_for_tests(Box::new(MockAiProvider::with_rounds(vec![
        vec![AiEvent::TextDelta("Antwort".to_string()), AiEvent::Done],
        vec![AiEvent::TextDelta("KI-Titel".to_string()), AiEvent::Done],
    ])));
    chat_store.rename_session(id, "Mein Titel").await.unwrap();
    run_first_turn(&session, "hallo").await;
    assert_eq!(
        title_of(&chat_store, &session).await.as_deref(),
        Some("Mein Titel")
    );
}

/// Issue #551: Sicherheitsnetz — Ende einer Sitzung ohne Titel, deren
/// erster Turn nie fertig wurde: Titel über dieselbe Logik.
#[tokio::test]
async fn test_issue_551_session_end_titles_untitled_session_with_user_message() {
    let (session, chat_store, _id, _tmp) = session_with_real_chat_persistence(
        vec![AiEvent::Error(ssh_manager_core::ai::AiError::RateLimited)],
        MockSshTransport::default(),
    )
    .await;
    push_history(
        &session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Nginx neu starten".to_string()),
        },
    )
    .await;
    generate_session_title_on_disconnect(&session, Uuid::new_v4(), &TestEmitter::default()).await;
    assert_eq!(
        title_of(&chat_store, &session).await.as_deref(),
        Some("Nginx neu starten")
    );
}

/// Issue #551: backfill titles untitled sessions with a user message only,
/// redacted, and a second run changes nothing.
#[tokio::test]
async fn test_issue_551_backfill_titles_untitled_sessions_idempotently() {
    let (session, chat_store, id, _tmp) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    chat_store
        .append_message(
            id,
            &ChatMessage {
                role: Role::User,
                content: MessageContent::Text(format!("pruefe password={SECRET_0096}   bitte")),
            },
        )
        .await
        .unwrap();
    let empty = chat_store
        .create_session(&session.server_id, None)
        .await
        .unwrap();
    let titled = chat_store
        .create_session(&session.server_id, None)
        .await
        .unwrap();
    chat_store
        .append_message(
            titled,
            &ChatMessage {
                role: Role::User,
                content: MessageContent::Text("egal".to_string()),
            },
        )
        .await
        .unwrap();
    chat_store.rename_session(titled, "Manuell").await.unwrap();

    for _ in 0..2 {
        crate::orchestration::backfill_missing_chat_titles(&chat_store).await;
        let listed = chat_store
            .list_sessions_for_server(&session.server_id)
            .await
            .unwrap();
        let title = |sid| listed.iter().find(|s| s.id == sid).unwrap().title.clone();
        let backfilled = title(id).expect("backfilled");
        assert_no_fragment_of_secret(&backfilled);
        assert!(
            backfilled.starts_with("pruefe [REDACTED] bitte"),
            "{backfilled}"
        );
        assert_eq!(title(empty), None);
        assert_eq!(title(titled).as_deref(), Some("Manuell"));
    }
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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyEchoPolicyStore)));
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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyEchoPolicyStore)));
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
    let mut session = session_with_ai_provider_allowing_everything(
        provider,
        MockSshTransport::default()
            .with_response("echo one", output("one"))
            .with_response("echo two", output("two")),
    );
    // Spec 0039: der Dialog in Runde 2 muss verlässlich auftauchen,
    // damit "Automatik stoppen" mitten im offenen Dialog überhaupt
    // testbar ist — `Strict` eskaliert jede Aktion, sobald das Flag
    // (durch die Ausführung von "echo one" in Runde 1) gesetzt ist,
    // unabhängig davon, ob "echo two" selbst als "verändernd" gilt.
    session.set_post_ingest_policy_for_tests(PostIngestPolicy::Strict);
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
    expect_event_within(
        &emitter,
        "chat-action-proposed (Confirm) für \"echo two\" in Runde 2, danach Turn-Ende nach Stopp der Automatik",
        async {
            tokio::join!(turn, responder);
        },
    )
    .await;

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

/// Spec 0022, Abschnitt 3, zweiter Punkt (Issue #249): das Sudo-Passwort
/// wird laut Spec 0018 einmalig bei `connect()` über
/// `read_sudo_password_for_redaction` gelesen und in `Session.sudo_password`
/// gecacht. Der Chat-Turn hat strukturell keinen Zugriff auf den
/// `CredentialStore`; diese Garantie wird nicht per Test wiederholt. Dieser
/// Test fährt den Produktions-Lesepfad und danach mehrere `sudo`-Kommandos
/// (über die automatische Fortsetzung aus Spec 0021) und zählt die
/// Store-Zugriffe: genau einer für den Lesepfad, keiner mehr für die
/// Kommandos, und das Passwort kommt tatsächlich bei der Ausführung an.
#[tokio::test]
async fn test_sudo_password_credential_store_not_read_again_across_multiple_commands() {
    let server_id = ServerId::new();
    let credential_ref = crate::server_credentials::sudo_password_credential_ref(server_id);
    let store =
        crate::test_support::InMemoryCredentialStore::new().with_secret(&credential_ref, "hunter2");

    let resolved_password =
        crate::server_redaction::read_sudo_password_for_redaction(&store, server_id);
    assert!(resolved_password.is_some());
    assert_eq!(store.get_calls(), 1);

    let mut session = session_with_ai_provider_allowing_everything(
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
    session.set_sudo_password_for_tests(resolved_password);

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
    expect_event_within(
        &emitter,
        "zwei chat-action-proposed (Confirm) für beide sudo-Kommandos, danach Turn-Ende",
        async {
            tokio::join!(turn, responder);
        },
    )
    .await;

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
        wait_for_event(&emitter, "chat-text-delta").await;
        session.request_auto_continue_stop();
    };
    expect_event_within(
        &emitter,
        "Stopp muss den hängenden Stream beenden, nicht auf sein Ende warten",
        async { tokio::join!(turn, stopper) },
    )
    .await;

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
    let session = session_with_ai_provider_allowing_everything(
        provider,
        MockSshTransport::default().with_response("echo gefaehrlich", output("x")),
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
    let stopper = async {
        wait_for_event(&emitter, "chat-text-delta").await;
        // Beides gleichzeitig: Stopp setzen UND den Tool-Call sofort
        // verfügbar machen — der Stopp muss gewinnen.
        session.request_auto_continue_stop();
        gate.notify_waiters();
    };
    expect_event_within(&emitter, "Turn muss nach Stopp enden", async {
        tokio::join!(turn, stopper)
    })
    .await;

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
    let (outcome, ()) = expect_event_within(
        &emitter,
        "Stopp in der Wartezeit muss die Runde sofort beenden",
        async { tokio::join!(round, stopper) },
    )
    .await;

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
    let session = session_with_ai_provider_allowing_everything(
        provider,
        MockSshTransport::default().with_response("echo one", output("one")),
    );
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
    // Issue #245: combining adjacent same-role messages happens only in the
    // provider request; the history keeps the queued text as its own entry.
    assert_eq!(round_two.iter().filter(|m| is_queued_text(m)).count(), 1);
    assert!(matches!(
        round_two[result_at].content,
        MessageContent::CommandResult { .. }
    ));
    assert_eq!(round_two[result_at].role, Role::ActionResult);
    assert!(has_event(&emitter, "chat-queued-messages-sent"));
    assert!(session.chat_turn.lock().unwrap().queued.is_empty());
}

/// Spec 0066, spec-reviewer-Fund: ein Stopp, der eintrifft, nachdem ein
/// AutoExec-Vorschlag schon abgeholt ist, aber bevor die Ausführung
/// beginnt, verhindert die Ausführung — nur für den eigenen Chat.
#[tokio::test]
async fn test_stop_prevents_auto_exec_that_has_not_started_yet() {
    let session = session_with_ai_provider_allowing_everything(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default().with_response("echo nie", output("SOLLTE-NIE-LAUFEN")),
    );
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
    let session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    for origin in [
        ActionOrigin::Internal,
        ActionOrigin::Mcp { client_name: None },
    ] {
        let (decision, payload) = expect_proposed_decision(
            &session,
            AiAction::SuggestCommand {
                command: "sudo -n /usr/lib/openssh/sftp-server".to_string(),
            },
            origin,
            "Dialog muss enden",
        )
        .await;
        assert!(
            matches!(&decision, Decision::Confirm { code, .. }
                if code == "FILTER_SFTP_SERVER_REQUIRES_CONFIRM"),
            "{payload}"
        );
    }

    let (decision, payload) = expect_proposed_decision(
        &session,
        AiAction::SuggestCommand {
            command: "ls -l /usr/lib/openssh/sftp-server".to_string(),
        },
        ActionOrigin::Internal,
        "Aktion muss enden",
    )
    .await;
    assert!(matches!(decision, Decision::AutoExec), "{payload}");
}

/// Zweite Review-Runde (Spec 0068, ERHÖHT): eine Secret-Lese-Aktion darf
/// das Injection-Verdachts-Flag nicht verbrauchen — sonst liefe die
/// eigentliche Folgeaktion wieder automatisch. Der Dialog zeigt bei
/// gesetztem Flag den Injection-Grund.
#[tokio::test]
async fn test_secret_read_does_not_consume_injection_suspicion() {
    let session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    session
        .injection_suspected
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (first, payload) = expect_proposed_decision(
        &session,
        AiAction::SuggestCommand {
            command: "cat .env".to_string(),
        },
        ActionOrigin::Internal,
        "Dialog muss enden",
    )
    .await;
    assert!(
        matches!(&first, Decision::Confirm { code, .. }
            if code == "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM"),
        "{payload}"
    );
    assert!(session
        .injection_suspected
        .load(std::sync::atomic::Ordering::SeqCst));

    let (second, payload) = expect_proposed_decision(
        &session,
        AiAction::SuggestCommand {
            command: "systemctl restart nginx".to_string(),
        },
        ActionOrigin::Internal,
        "Dialog muss enden",
    )
    .await;
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
    let session =
        test_session_allowing_everything(vec![AiEvent::Done], MockSshTransport::default());
    let (decision, payload) = expect_proposed_decision(
        &session,
        AiAction::SuggestCommand {
            command: "cat /etc//shadow".to_string(),
        },
        ActionOrigin::Mcp { client_name: None },
        "Dialog muss enden",
    )
    .await;
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

/// Spec 0068, Teil 2: Secret-Pfad-Lesen wird trotz Allow-Regel nie
/// automatisch ausgeführt — Chat-Kommando, `read_remote_file` und
/// MCP-Herkunft (s. auch den MCP-Test oben). Ein öffentlicher Schlüssel bleibt automatisch.
///
/// Spec 0092: Für **einen** Fall (`cat ~/.ssh/id_rsa.pub`) ist die
/// Rot-Risiko-Einstellung ausgeschaltet. Grund: Er ist kein
/// Secret-Pfad (die Aussage dieses Tests), der Risiko-Klassifizierer stuft
/// es auf der Daten-Achse aber rot ein (Muster `id_rsa`, ohne
/// `.pub`-Ausnahme) — mit eingeschalteter Einstellung eskaliert es deshalb
/// über das neue Glied, mit dessen eigenem Code. Die Aussage hier bleibt
/// dadurch unverändert scharf („das SECRET-Glied greift nicht"), und das
/// neue Verhalten hat seinen eigenen Test
/// (`test_red_risk_escalates_a_public_key_read_that_is_no_secret_path`).
#[tokio::test]
async fn test_secret_path_read_always_requires_confirm_even_with_allow_rule() {
    // Drittes Feld (Spec 0092): die Rot-Risiko-Einstellung für diesen Fall.
    // Sie steht überall auf `true` — nur für `id_rsa.pub` nicht, weil der
    // Klassifizierer genau dieses Kommando auf der Daten-Achse rot einstuft
    // (Muster `id_rsa`, ohne `.pub`-Ausnahme) und das neue Glied es deshalb
    // eskalieren würde. Nur dieser eine Fall wird ausgenommen, nicht alle
    // negativen (spec-reviewer-Fund, Runde 1).
    let cases: Vec<(AiAction, bool, bool)> = vec![
        (
            AiAction::SuggestCommand {
                command: "cat ~/.ssh/id_rsa".to_string(),
            },
            true,
            true,
        ),
        (
            AiAction::SuggestCommand {
                command: "echo ok; sudo tail /srv/app/.env".to_string(),
            },
            true,
            true,
        ),
        (
            AiAction::ReadRemoteFile {
                path: "/root/.ssh/id_ed25519".to_string(),
            },
            true,
            true,
        ),
        (
            AiAction::SuggestCommand {
                command: "cat ~/.ssh/id_rsa.pub".to_string(),
            },
            false,
            false,
        ),
        (
            AiAction::SuggestCommand {
                command: "cp ~/.ssh/id_rsa /backup/id_rsa".to_string(),
            },
            false,
            true,
        ),
    ];
    for (action, expect_confirm, red_risk_always_confirm) in cases {
        let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
        session
            .set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowEverythingPolicyStore)));
        session.set_red_risk_always_confirm_for_tests(red_risk_always_confirm);
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
        expect_event_within(&emitter, "Aktion muss enden", async {
            tokio::join!(handled, responder)
        })
        .await;

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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyCatPolicyStore)));

    let (decision, payload) = expect_proposed_decision(
        &session,
        AiAction::SuggestCommand {
            command: "cat ~/.ssh/id_rsa".to_string(),
        },
        ActionOrigin::Internal,
        "Deny darf keinen Dialog öffnen",
    )
    .await;

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
    with_password
        .set_sudo_password_for_tests(Some(secrecy::SecretString::from("hunter2".to_string())));
    with_password
        .set_sftp_for_tests(Box::new(MockSftpSession::new()))
        .await;
    let (decision, payload) = expect_proposed_decision(
        &with_password,
        write.clone(),
        ActionOrigin::Internal,
        "Dialog muss enden",
    )
    .await;
    assert!(matches!(decision, Decision::Confirm { .. }), "{payload}");
    assert_eq!(payload["usesStoredSudoPassword"], true, "{payload}");

    let without = test_session(vec![AiEvent::Done], MockSshTransport::default());
    without
        .set_sftp_for_tests(Box::new(MockSftpSession::new()))
        .await;
    let (_, payload) =
        expect_proposed_decision(&without, write, ActionOrigin::Internal, "Dialog muss enden")
            .await;
    assert_eq!(payload["usesStoredSudoPassword"], false, "{payload}");
}

/// Wie oben, über MCP (Review-Fund): derselbe Hinweis im Dialog.
#[tokio::test]
async fn test_mcp_write_confirmation_announces_possible_sudo_fallback() {
    let mut session = test_session(vec![AiEvent::Done], MockSshTransport::default());
    session.set_sudo_password_for_tests(Some(secrecy::SecretString::from("hunter2".to_string())));
    session
        .set_sftp_for_tests(Box::new(MockSftpSession::new()))
        .await;
    let (decision, payload) = expect_proposed_decision(
        &session,
        AiAction::WriteRemoteFile {
            path: "/etc/nginx/nginx.conf".to_string(),
            content: "worker_processes 2;".to_string(),
        },
        ActionOrigin::Mcp { client_name: None },
        "Dialog muss enden",
    )
    .await;
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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowLsOnly)));
    session.set_post_ingest_policy_for_tests(PostIngestPolicy::Standard);
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    expect_event_within(
        &emitter,
        "Turn-Ende: run_chat_turn kehrt zurück, alle Dialoge beantwortet",
        async {
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
        },
    )
    .await;

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
    let mut session = test_session_allowing_everything(
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
    session.set_post_ingest_policy_for_tests(PostIngestPolicy::Balanced);
    assert!(!session
        .untrusted_content_ingested
        .load(std::sync::atomic::Ordering::SeqCst));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    expect_event_within(
        &emitter,
        "Turn-Ende: run_chat_turn kehrt zurück, alle Dialoge beantwortet",
        async {
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
        },
    )
    .await;

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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowLsOnly)));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    expect_event_within(
        &emitter,
        "Turn-Ende: run_chat_turn kehrt zurück, alle Dialoge beantwortet",
        async {
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
        },
    )
    .await;

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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowLsOnly)));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    expect_event_within(
        &emitter,
        "Turn-Ende: run_chat_turn kehrt zurück, alle Dialoge beantwortet",
        async {
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
        },
    )
    .await;

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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyRmAllowLs2)));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    expect_event_within(
        &emitter,
        "Turn-Ende: run_chat_turn kehrt zurück, alle Dialoge beantwortet",
        async {
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
        },
    )
    .await;

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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(DenyRmAllowLs)));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    expect_event_within(
        &emitter,
        "Turn-Ende: run_chat_turn kehrt zurück, alle Dialoge beantwortet",
        async {
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
        },
    )
    .await;

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
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowLsOnly)));
    session.set_post_ingest_policy_for_tests(PostIngestPolicy::Standard);
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
    expect_event_within(
        &emitter,
        "Turn-Ende: run_chat_turn kehrt zurück, alle Dialoge beantwortet",
        async {
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
        },
    )
    .await;

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

// --- Spec 0096, A1: generierter Sitzungstitel wird geschwärzt ----------

/// Das Geheimnis, mit dem Spec 0096 arbeitet. Die Form, in der der
/// Session-Redactor es erkennt (`password=Geheim-0096` → `[REDACTED]`),
/// ist gemessen in `orchestration::notes::tests` festgehalten.
const SECRET_0096: &str = "Geheim-0096";

/// Erzeugt einen Titel aus `ai_title`, nachdem eine Nutzer-Nachricht in der
/// Historie liegt (Auslösebedingung von
/// `generate_session_title_on_disconnect`), und liefert den tatsächlich in
/// `chat_sessions.title` gespeicherten Titel zurück.
async fn generated_title_for(ai_title: &str) -> Option<String> {
    let (session, chat_store, _chat_session_id, _tmp_dir) = session_with_real_chat_persistence(
        vec![AiEvent::TextDelta(ai_title.to_string()), AiEvent::Done],
        MockSshTransport::default(),
    )
    .await;
    push_history(
        &session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("was war hier los?".to_string()),
        },
    )
    .await;

    generate_session_title_on_disconnect(&session, Uuid::new_v4(), &TestEmitter::default()).await;

    chat_store
        .list_sessions_for_server(&session.server_id)
        .await
        .unwrap()[0]
        .title
        .clone()
}

/// Prüft, dass im Titel **kein Bruchstück** des Geheimnisses steht — nicht
/// nur die vollständige Zeichenfolge. Ein durch das Kürzen auf 60 Zeichen
/// angeschnittenes Muster hinterließe genau so ein Bruchstück (Spec 0096,
/// Angriffsrichtung zu T6), und `!contains(SECRET_0096)` allein würde das
/// durchgehen lassen.
fn assert_no_fragment_of_secret(title: &str) {
    for len in 1..=SECRET_0096.chars().count() {
        let prefix: String = SECRET_0096.chars().take(len).collect();
        assert!(
            !title.contains(&prefix),
            "Titel {title:?} enthält das Geheimnis-Bruchstück {prefix:?}"
        );
    }
}

/// T1 (Spec 0096, A1): Die KI übernimmt ein Geheimnis in den Titel — der
/// Titel ist eine **Klartext**-Spalte (`chat_sessions.title`, Spec §1),
/// also darf dort nichts davon ankommen. Scheitert gegen den Stand vor
/// Spec 0096 (dort wurde `text_buffer` ungeschwärzt gespeichert).
#[tokio::test]
async fn test_t1_spec_0096_generated_title_is_redacted_before_storing() {
    let title = generated_title_for("Login mit password=Geheim-0096 geprueft")
        .await
        .expect("Rest-Text neben dem Platzhalter -> Titel wird gespeichert");

    assert_no_fragment_of_secret(&title);
    assert!(
        title.contains("[REDACTED]"),
        "der Platzhalter muss den Treffer ersetzen, statt ihn stumm zu entfernen: {title:?}"
    );
}

/// T2 (Spec 0096, A1, letzter Satz): Besteht der Titel nach dem Schwärzen
/// nur noch aus Platzhalter und Leerraum, wird gar kein Titel gespeichert —
/// ein Titel `[REDACTED]` wäre für den Nutzer wertlos.
#[tokio::test]
async fn test_t2_spec_0096_title_of_only_placeholders_is_not_stored() {
    // Issue #551: statt gar keinem Titel greift jetzt der Ersatztitel aus
    // der ersten Nutzer-Nachricht ("was war hier los?") — nie der
    // Platzhalter und nie das Geheimnis.
    for ai_title in ["password=Geheim-0096", "  \"password=Geheim-0096\"  "] {
        assert_eq!(
            generated_title_for(ai_title).await.as_deref(),
            Some("was war hier los?")
        );
    }
}

/// T6 (Spec 0096, A1, Angriffsrichtung „Titel wird vor dem Redactor gekürzt
/// und schneidet ein Muster an"): Das Muster reicht über Position 60
/// (`MAX_GENERATED_TITLE_LENGTH`) hinaus, **der Anfang des Geheimniswerts
/// liegt aber noch davor**. Würde erst gekürzt und dann geschwärzt, sähe der
/// Redactor nur `password=Gehei` — kein vollständiges Muster mehr, der
/// Anfang des Geheimnisses bliebe im Klartext im Titel stehen.
///
/// **Klarstellung zu Spec 0096 §7, T6 (Coder):** Die Spec nennt 55 Zeichen
/// Fülltext. Damit beginnt `password=` erst an Position 55, und das Kürzen
/// bei 60 schneidet schon im Schlüsselwort (`…passw`) — der Wert selbst
/// erreicht den Titel gar nicht, der Test könnte also auch mit vertauschter
/// Reihenfolge nicht scheitern (gemessen). Geprüft werden deshalb **beide**
/// Längen: die aus der Spec als Grenzfall und eine, bei der der Wert
/// tatsächlich über die Grenze läuft (45 Zeichen → Wert ab Position 55).
/// Nur Letztere trägt den Gegenbeweis.
#[tokio::test]
async fn test_t6_spec_0096_title_truncation_cannot_cut_a_pattern_open() {
    for filler_len in [45, 55] {
        let filler = "a".repeat(filler_len);
        let title = generated_title_for(&format!("{filler} password=Geheim-0096"))
            .await
            .expect("der Fuelltext bleibt als Titel uebrig");

        assert_no_fragment_of_secret(&title);
        assert!(
            title.chars().count() <= 60,
            "die 60-Zeichen-Grenze gilt weiterhin: {title:?}"
        );
        assert!(title.starts_with(&filler), "Fuelltext bleibt erhalten");
    }

    // Das Muster mit **nachlaufendem Anker**: die URL-Zugangsdaten-Regel
    // braucht das `@host` hinter dem Passwort. Liegt der Anker jenseits von
    // Position 60, erkennt der Redactor nach einem vorgezogenen Kürzen gar
    // nichts mehr — und das Geheimnis stünde vollständig im Titel. Dieser
    // Fall trägt den Gegenbeweis (s. Doc-Kommentar).
    let filler = "a".repeat(40);
    let title = generated_title_for(&format!("{filler} https://u:Geheim-0096@example.invalid/x"))
        .await
        .expect("der Fuelltext bleibt als Titel uebrig");
    assert_no_fragment_of_secret(&title);
}

/// T7 (Spec 0096, A1): mehrere Muster in einem Titel, das Geheimnis
/// zusätzlich in Anführungszeichen — kein Treffer bleibt stehen.
#[tokio::test]
async fn test_t7_spec_0096_title_with_several_patterns_keeps_no_secret() {
    let title = generated_title_for("Setup token=Geheim-0096 und password=\"Geheim-0096\" ok")
        .await
        .expect("Rest-Text neben den Platzhaltern -> Titel wird gespeichert");

    assert_no_fragment_of_secret(&title);
}

/// spec-reviewer-Fund (Runde 1, Spec 0096 A2): Ein vollständig geschwärzter
/// Notizvorschlag wird verworfen, ohne `handle_action_proposed` zu
/// durchlaufen — und damit auch ohne die Stelle, die sonst
/// `earlier_rejection` setzt (Spec 0068, Teil 4). Ohne die Nachbesserung
/// liefe eine per Allow-Regel freigegebene Aktion **derselben KI-Antwort**
/// danach automatisch durch.
///
/// Dass die KI gerade einen Vorschlag geliefert hat, der komplett aus
/// erkannten Zugangsdaten bestand, ist mindestens so verdächtig wie eine
/// Ablehnung durch den Nutzer — die Antwort muss dieselbe sein: eigener
/// Dialog statt stiller Ausführung.
#[tokio::test]
async fn test_spec_0096_discarded_note_proposal_escalates_allowed_later_action() {
    let mut session = test_session(
        vec![
            AiEvent::ActionProposed(AiAction::ProposeNoteUpdate {
                target: ssh_manager_core::profiles::NoteTargetSelector::CurrentServer,
                new_content: "password=Geheim-0096".to_string(),
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            }),
            AiEvent::Done,
        ],
        MockSshTransport::default().with_response("ls -la", output("a")),
    );
    // `AllowLsOnly` gibt `ls *` per Regel frei — ohne den vorangegangenen
    // verworfenen Vorschlag liefe `ls -la` als `AutoExec` durch.
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(AllowLsOnly)));
    let emitter = TestEmitter::default();
    let confirmations = ConfirmationRegistry::new();
    let profile_store = InMemoryProfileStore::default();

    expect_event_within(
        &emitter,
        "Turn-Ende: run_chat_turn kehrt zurück, alle Dialoge beantwortet",
        async {
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
        },
    )
    .await;

    let decisions = proposed_decisions(&emitter);
    assert_eq!(
        decisions.len(),
        1,
        "der verworfene Notizvorschlag erzeugt keine Karte, `ls -la` schon: {decisions:?}"
    );
    assert_eq!(decisions[0].0, "ls -la");
    assert_eq!(
        decisions[0].1["Confirm"]["code"], "FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM",
        "die erlaubte Folgeaktion muss eskaliert werden: {decisions:?}"
    );
    let history = session.context.lock().await.history.clone();
    assert!(
        executed_commands(&history).is_empty(),
        "nichts darf automatisch gelaufen sein: {history:?}"
    );
}

/// Issue #562: extension that resolves the first `ok_calls` requests and
/// fails afterwards, so sealing succeeds for the answer and fails for the
/// title request that follows.
struct FlakyExt {
    calls: std::sync::atomic::AtomicUsize,
    ok_calls: usize,
}

impl crate::chat_content_extension::ChatContentExtension for FlakyExt {
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
    fn persist(
        &self,
        _s: Uuid,
        _part_id: &str,
        _b: Vec<ssh_manager_core::ai::RedactedBlock>,
    ) -> Result<(), crate::chat_content_extension::ChatContentError> {
        Ok(())
    }
    fn resolve(
        &self,
        _s: Uuid,
        _part: &ssh_manager_core::ai::ExtensionPart,
    ) -> Result<
        Option<Vec<ssh_manager_core::ai::ResolvedBlock>>,
        crate::chat_content_extension::ChatContentError,
    > {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n < self.ok_calls {
            Ok(Some(vec![ssh_manager_core::ai::ResolvedBlock::Text {
                text: "attachment".into(),
            }]))
        } else {
            Err(crate::chat_content_extension::ChatContentError(
                "gone".into(),
            ))
        }
    }
}

/// Issue #562: when sealing fails for the title request, no title request is
/// made and the redacted first-message title is used.
#[tokio::test]
async fn test_issue_562_title_request_not_sent_when_sealing_fails() {
    let (mut session, chat_store, _id, _tmp) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    let provider = MockAiProvider::with_rounds(vec![
        vec![AiEvent::TextDelta("Antwort".to_string()), AiEvent::Done],
        vec![
            AiEvent::TextDelta("Should not be used".to_string()),
            AiEvent::Done,
        ],
    ]);
    let contexts = provider.received_contexts_handle();
    session.set_ai_provider_for_tests(Box::new(provider));
    session.set_chat_content_for_tests(Arc::new(
        crate::chat_content_extension::ChatContentRegistry::new(vec![Arc::new(FlakyExt {
            calls: std::sync::atomic::AtomicUsize::new(0),
            ok_calls: 1,
        })]),
    ));
    push_history(
        &session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("Platte prüfen".to_string()),
        },
    )
    .await;
    push_history(
        &session,
        ChatMessage {
            role: Role::User,
            content: MessageContent::Extension(ssh_manager_core::ai::ExtensionPart {
                extension: "files".into(),
                kind: "doc".into(),
                id: Uuid::new_v4().to_string(),
                label: "a.txt".into(),
                media: ssh_manager_core::ai::ExtensionMedia::Text,
                estimated_tokens: 1,
            }),
        },
    )
    .await;
    run_chat_turn(
        &session,
        Uuid::new_v4(),
        &TestEmitter::default(),
        &InMemoryProfileStore::default(),
        &ConfirmationRegistry::new(),
    )
    .await;
    assert_eq!(contexts.lock().unwrap().len(), 1, "answer only, no title");
    assert_eq!(
        title_of(&chat_store, &session).await.as_deref(),
        Some("Platte prüfen")
    );
}

/// Issue #562: for a plain session the title request is unchanged, and a
/// planted secret reaches neither the request nor the stored title.
#[tokio::test]
async fn test_issue_562_title_request_plain_session_and_secret_absent() {
    let (mut session, chat_store, _id, _tmp) =
        session_with_real_chat_persistence(vec![AiEvent::Done], MockSshTransport::default()).await;
    let provider = MockAiProvider::with_rounds(vec![
        vec![AiEvent::TextDelta("Antwort".to_string()), AiEvent::Done],
        vec![
            AiEvent::TextDelta(format!("Titel password={SECRET_0096}")),
            AiEvent::Done,
        ],
    ]);
    let contexts = provider.received_contexts_handle();
    session.set_ai_provider_for_tests(Box::new(provider));
    run_first_turn(&session, &format!("prüfe password={SECRET_0096} bitte")).await;

    let contexts = contexts.lock().unwrap().clone();
    assert_eq!(contexts.len(), 2);
    let title_request = &contexts[1];
    let texts: Vec<String> = title_request
        .history
        .iter()
        .map(|m| match &m.content {
            MessageContent::Text(t) => t.clone(),
            other => panic!("plain session must only carry text: {other:?}"),
        })
        .collect();
    assert_eq!(texts.len(), 3, "user, answer, instruction");
    assert!(texts[0].starts_with("prüfe"));
    assert_eq!(texts[1], "Antwort");
    for t in &texts {
        assert_no_fragment_of_secret(t);
    }
    let title = title_of(&chat_store, &session).await.expect("title");
    assert_no_fragment_of_secret(&title);
}
