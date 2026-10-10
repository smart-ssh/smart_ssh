//! Spec 0088 — das Bestätigungswarten räumt auf jedem Ausgang ab (A1), und
//! zwar auch bei vergifteter Sperre und ohne selbst zu panicken (A2).
//!
//! Alle Tests hier laufen unter einer Zeitgrenze: Schlägt der Fix fehl, soll
//! der Testlauf rot werden statt zu blockieren.

use std::sync::Arc;
use std::time::Duration;

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::profiles::AiAction;

use crate::confirmation::ConfirmationRegistry;
use crate::dto::{ActionOrigin, ActionUserDecision};
use crate::events::{EventEmitter, TestEmitter};
use crate::poison::{lock_tolerating_poison, poison_for_test};
use crate::session::{Session, SessionManager};
use crate::state::{ActionId, SessionId};

use super::super::test_support::{
    output, session_with_ai_provider, session_with_second_opinion, test_fresh_rejection_flag,
    test_session, MockAiProvider, MockSshTransport,
};
use super::handle_action_proposed;

/// Obergrenze für jedes Warten in dieser Datei — großzügig genug für
/// langsame CI-Läufer, klein genug, dass ein Hänger als Testfehler endet
/// statt den Lauf zu blockieren.
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

const TEST_COMMAND: &str = "rm -rf /data";

/// Alles, was `handle_action_proposed` braucht, in `Arc`s — nötig, weil die
/// Abbruch-Tests die Aufrufkette per `tokio::spawn` in eine eigene Task
/// legen müssen (nur dort gibt es ein `abort()` und einen `JoinError`).
struct Fixture {
    session: Arc<Session>,
    emitter: Arc<TestEmitter>,
    profile_store: Arc<crate::orchestration::test_support::InMemoryProfileStore>,
    confirmations: Arc<ConfirmationRegistry<ActionId, ActionUserDecision>>,
    session_id: SessionId,
}

impl Fixture {
    fn with_session(session: Session) -> Self {
        Self {
            session: Arc::new(session),
            emitter: Arc::new(TestEmitter::default()),
            profile_store: Arc::new(Default::default()),
            confirmations: Arc::new(ConfirmationRegistry::new()),
            session_id: uuid::Uuid::new_v4(),
        }
    }

    /// Ein Vorschlag, der ohne passende Allow-Regel auf `Confirm` landet
    /// (`NoRulesPolicyStore` ist der `test_session`-Standard).
    fn new() -> Self {
        Self::with_session(test_session(
            vec![AiEvent::Done],
            MockSshTransport::default(),
        ))
    }

    fn spawn_proposal(&self) -> tokio::task::JoinHandle<bool> {
        self.spawn_proposal_with_emitter(self.emitter.clone())
    }

    fn spawn_proposal_with_emitter(
        &self,
        emitter: Arc<dyn EventEmitter>,
    ) -> tokio::task::JoinHandle<bool> {
        let session = self.session.clone();
        let profile_store = self.profile_store.clone();
        let confirmations = self.confirmations.clone();
        let session_id = self.session_id;
        tokio::spawn(async move {
            handle_action_proposed(
                &session,
                session_id,
                AiAction::SuggestCommand {
                    command: TEST_COMMAND.to_string(),
                },
                emitter.as_ref(),
                profile_store.as_ref(),
                confirmations.as_ref(),
                ActionOrigin::Internal,
                test_fresh_rejection_flag(),
            )
            .await
        })
    }

    /// Der Indikator, vergiftungstolerant gelesen — die Tests zu A2 lesen
    /// ihn, nachdem sie die Sperre absichtlich vergiftet haben.
    fn pending_action(&self) -> Option<ActionId> {
        *lock_tolerating_poison(&self.session.pending_action)
    }

    fn proposed_action_id(&self) -> Option<ActionId> {
        proposed_action_id_of(&self.emitter)
    }

    fn event_count(&self, name: &str) -> usize {
        self.emitter
            .events
            .lock()
            .expect("Emitter-Sperre ist nicht vergiftet")
            .iter()
            .filter(|(event, _)| event == name)
            .count()
    }

    /// Wartet, bis der Tab-Indikator gesetzt ist — also bis die
    /// Bestätigungs-Task tatsächlich im `await` hängt.
    async fn await_pending_indicator(&self) -> ActionId {
        with_timeout("Indikator wurde nie gesetzt", async {
            loop {
                if let Some(id) = self.pending_action() {
                    return id;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
    }

    /// Meldet die Sitzung beim `SessionManager` an, damit der Snapshot
    /// (Tab-Leiste) geprüft werden kann.
    fn manager(&self) -> SessionManager {
        let manager = SessionManager::new();
        manager.insert(self.session_id, self.session.clone());
        manager
    }

    fn snapshot_has_pending_action(&self, manager: &SessionManager) -> bool {
        manager
            .snapshot()
            .into_iter()
            .find(|entry| entry.session_id == self.session_id)
            .expect("die Sitzung ist angemeldet")
            .has_pending_action
    }
}

fn proposed_action_id_of(emitter: &TestEmitter) -> Option<ActionId> {
    emitter
        .events
        .lock()
        .expect("Emitter-Sperre ist nicht vergiftet")
        .iter()
        .find_map(|(name, payload)| {
            (name == "chat-action-proposed").then(|| {
                payload["actionId"]
                    .as_str()
                    .expect("actionId ist ein String")
                    .parse()
                    .expect("actionId ist eine UUID")
            })
        })
}

async fn with_timeout<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    match tokio::time::timeout(TEST_TIMEOUT, future).await {
        Ok(value) => value,
        Err(_) => panic!("{what} (Zeitgrenze {TEST_TIMEOUT:?} überschritten)"),
    }
}

/// T1 (A1.1, gegen `33d0501` rot): Der Tab-Indikator „wartet auf
/// Bestätigung" verschwindet, wenn die wartende Task abgebrochen wird.
///
/// Vor Spec 0088 hing das Zurücksetzen von `pending_action` an einer
/// Anweisung HINTER dem `await` — die ein Abbruch nie erreicht. Der
/// Indikator blieb dann für den Rest der Sitzung stehen, ohne dass es noch
/// eine Aktion gegeben hätte, die man hätte bestätigen können.
#[tokio::test]
async fn test_aborting_the_waiting_task_clears_the_pending_action_indicator() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let task = fixture.spawn_proposal();

    let action_id = fixture.await_pending_indicator().await;
    assert!(
        fixture.snapshot_has_pending_action(&manager),
        "während des Wartens muss der Tab-Indikator stehen"
    );

    task.abort();
    let join_error = with_timeout("die abgebrochene Task endete nicht", task)
        .await
        .expect_err("eine abgebrochene Task liefert keinen Wert");
    assert!(join_error.is_cancelled());

    assert_eq!(
        fixture.pending_action(),
        None,
        "nach dem Abbruch darf kein Indikator für {action_id} stehen bleiben"
    );
    assert!(
        !fixture.snapshot_has_pending_action(&manager),
        "der Snapshot für die Tab-Leiste muss has_pending_action=false melden"
    );
}

/// T2 (A1.2, gegen `33d0501` rot): Derselbe Abbruch räumt auch den Eintrag
/// in der Registry ab — sonst bliebe ein toter `Sender` liegen, auf den
/// niemand mehr wartet.
#[tokio::test]
async fn test_aborting_the_waiting_task_removes_the_registry_entry() {
    let fixture = Fixture::new();
    let task = fixture.spawn_proposal();
    let action_id = fixture.await_pending_indicator().await;
    assert!(
        fixture.confirmations.contains(&action_id),
        "während des Wartens muss der Eintrag existieren"
    );

    task.abort();
    let _ = with_timeout("die abgebrochene Task endete nicht", task).await;

    assert!(
        !fixture.confirmations.contains(&action_id),
        "nach dem Abbruch darf kein Registry-Eintrag für {action_id} bleiben"
    );
}

/// T3 (A1.3, Regression): Abgeräumt heißt nie genehmigt. Eine Genehmigung,
/// die nach dem Abbruch eintrifft, scheitert und führt nichts aus.
#[tokio::test]
async fn test_approving_after_an_abort_fails_and_executes_nothing() {
    let fixture = Fixture::new();
    let task = fixture.spawn_proposal();
    let action_id = fixture.await_pending_indicator().await;

    task.abort();
    let _ = with_timeout("die abgebrochene Task endete nicht", task).await;

    let result = fixture
        .confirmations
        .resolve(&action_id, ActionUserDecision::Approve);
    assert!(
        result.is_err(),
        "eine Genehmigung für eine abgeräumte Aktion muss scheitern, \
         nicht stillschweigend etwas auslösen"
    );

    // Kurz weiterlaufen lassen, damit eine (fälschlich) losgetretene
    // Ausführung ihr Ergebnis noch senden könnte.
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        fixture.event_count("chat-action-result"),
        0,
        "nach dem Abbruch darf kein Kommando mehr ausgeführt worden sein"
    );
}

/// T4 (A1.2, adversarial, gegen `33d0501` rot): Der Abbruch trifft die Task
/// **während der Zweitmeinung** — also nachdem registriert wurde, aber bevor
/// der Indikator überhaupt gesetzt ist.
///
/// Scheitert gegen einen Guard, dessen Aufräumen erst mit dem Setzen des
/// Indikators beginnt: Der Registry-Eintrag bliebe dann liegen.
#[tokio::test]
async fn test_aborting_during_the_second_opinion_removes_the_registry_entry() {
    /// Ein Zweitmeinungs-Provider, dessen Stream nie etwas liefert — die
    /// Task hängt damit zuverlässig im Zweitmeinungs-`await`.
    struct HangingProvider;
    impl ssh_manager_core::ai::AiProvider for HangingProvider {
        fn send(
            &self,
            _context: ssh_manager_core::ai::SessionContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
            Box::pin(futures::stream::pending())
        }
    }

    let fixture = Fixture::with_session(session_with_second_opinion(
        vec![AiEvent::Done],
        MockSshTransport::default(),
        HangingProvider,
    ));
    let task = fixture.spawn_proposal();

    // Warten, bis der Vorschlag raus ist — der liegt zwischen Registrierung
    // und Zweitmeinung.
    let action_id = with_timeout("der Vorschlag wurde nie gesendet", async {
        loop {
            if let Some(id) = fixture.proposed_action_id() {
                return id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(
        fixture.pending_action(),
        None,
        "der Indikator darf während der Zweitmeinung noch nicht stehen — \
         sonst prüft dieser Test nicht mehr, was er prüfen soll"
    );
    assert!(fixture.confirmations.contains(&action_id));

    task.abort();
    let _ = with_timeout("die abgebrochene Task endete nicht", task).await;

    assert!(
        !fixture.confirmations.contains(&action_id),
        "ein Abbruch während der Zweitmeinung muss den Registry-Eintrag \
         genauso abräumen wie einer während des Wartens"
    );
    assert_eq!(fixture.pending_action(), None);
}

/// T5 (A2.1, adversarial, gegen `33d0501` rot): Ist die Sperre von
/// `pending_action` vergiftet, laufen Snapshot, Vorschlag und Genehmigung
/// trotzdem durch — und die Aktion wird genau einmal ausgeführt.
///
/// Scheitert auch gegen einen Guard, der bei Vergiftung das Abräumen
/// auslässt: Der Indikator bliebe am Ende stehen.
#[tokio::test]
async fn test_a_poisoned_pending_action_lock_breaks_neither_snapshot_nor_confirmation() {
    let fixture = Fixture::with_session(session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default().with_response(TEST_COMMAND, output("weg")),
    ));
    poison_for_test(&fixture.session.pending_action);

    let manager = fixture.manager();
    assert!(
        !fixture.snapshot_has_pending_action(&manager),
        "der Snapshot muss über eine vergiftete Sperre hinweg antworten, \
         statt zu panicken"
    );

    let task = fixture.spawn_proposal();
    let action_id = fixture.await_pending_indicator().await;
    assert!(fixture.snapshot_has_pending_action(&manager));

    fixture
        .confirmations
        .resolve(&action_id, ActionUserDecision::Approve)
        .expect("die Genehmigung muss den Wartenden erreichen");

    let executed = with_timeout("der Vorschlag wurde nie fertig", task)
        .await
        .expect("die Task darf nicht panicken");
    assert!(executed, "die genehmigte Aktion muss ausgeführt werden");
    assert_eq!(
        fixture.event_count("chat-action-result"),
        1,
        "genau eine Ausführung"
    );
    assert_eq!(
        fixture.pending_action(),
        None,
        "auch bei vergifteter Sperre muss der Indikator danach weg sein"
    );
    assert!(!fixture.snapshot_has_pending_action(&manager));
}

/// T6a (A2.2, adversarial): Ein Panic im Wartepfad bei bereits vergifteter
/// Sperre darf nicht zu einem **zweiten** Panic im `Drop` führen — der bräche
/// während des Abwickelns den ganzen Prozess ab, statt nur diese Task.
///
/// Dass der Test überhaupt bis zu seiner Assertion kommt, ist die
/// eigentliche Aussage: Bei einem Prozessabbruch gäbe es keine Ausgabe mehr.
#[tokio::test]
async fn test_a_panic_in_the_wait_path_does_not_abort_the_process_when_the_lock_is_poisoned() {
    /// Panickt beim Vorschlags-Event — also nach der Registrierung, mitten
    /// im Lebensbereich des Guards. Die `action_id` wird vorher festgehalten,
    /// damit der Test danach noch weiß, wonach er suchen muss.
    struct PanicOnProposedEmitter {
        seen: std::sync::Mutex<Option<ActionId>>,
    }
    impl EventEmitter for PanicOnProposedEmitter {
        fn emit_event(&self, event: &str, payload: serde_json::Value) {
            if event == "chat-action-proposed" {
                *self.seen.lock().expect("nicht vergiftet") = Some(
                    payload["actionId"]
                        .as_str()
                        .expect("actionId ist ein String")
                        .parse()
                        .expect("actionId ist eine UUID"),
                );
                panic!("Testpanic (Spec 0088, T6a) — erwartet");
            }
        }
    }

    let fixture = Fixture::new();
    poison_for_test(&fixture.session.pending_action);

    let emitter = Arc::new(PanicOnProposedEmitter {
        seen: std::sync::Mutex::new(None),
    });
    let task = fixture.spawn_proposal_with_emitter(emitter.clone());

    let join_error = with_timeout("die panickende Task endete nicht", task)
        .await
        .expect_err("die Task muss gepanickt sein");
    assert!(
        join_error.is_panic(),
        "erwartet wird genau ein Panic — der im Emitter, nicht ein zweiter im Drop"
    );

    let action_id = emitter
        .seen
        .lock()
        .expect("nicht vergiftet")
        .expect("der Emitter hat das Vorschlags-Event gesehen");
    assert!(
        !fixture.confirmations.contains(&action_id),
        "auch ein Panic muss den Registry-Eintrag abräumen"
    );
    assert_eq!(fixture.pending_action(), None);
}

/// T6b (A2.2, adversarial, rot gegen ein `Drop` mit `lock().unwrap()`):
/// Abbruch bei vergifteter Sperre.
///
/// tokio meldet einen Panic im `Drop` einer abgebrochenen Task als
/// `is_panic()` statt `is_cancelled()` (`runtime/task/harness.rs`,
/// `cancel_task`). Genau daran scheitert dieser Test, wenn der Guard die
/// Sperre mit `unwrap` nimmt.
#[tokio::test]
async fn test_aborting_with_a_poisoned_lock_cancels_instead_of_panicking() {
    let fixture = Fixture::new();
    poison_for_test(&fixture.session.pending_action);

    let task = fixture.spawn_proposal();
    let _action_id = fixture.await_pending_indicator().await;

    task.abort();
    let join_error = with_timeout("die abgebrochene Task endete nicht", task)
        .await
        .expect_err("eine abgebrochene Task liefert keinen Wert");
    assert!(
        !join_error.is_panic(),
        "der Drop darf bei vergifteter Sperre nicht panicken"
    );
    assert!(join_error.is_cancelled());
    assert_eq!(fixture.pending_action(), None);
}

/// T14 (A1.1, adversarial): Der Guard räumt **nicht zu früh** ab.
///
/// Scheitert gegen einen Guard, der sofort fällt (etwa weil er an `let _ =
/// …` gebunden würde): Dann wären Indikator und Registry-Eintrag schon weg,
/// bevor der Nutzer antworten kann — und die Genehmigung liefe ins Leere.
#[tokio::test]
async fn test_the_indicator_and_registry_entry_survive_until_the_user_answers() {
    let fixture = Fixture::with_session(session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default().with_response(TEST_COMMAND, output("weg")),
    ));
    let task = fixture.spawn_proposal();

    let action_id = fixture.await_pending_indicator().await;
    assert_eq!(
        fixture.pending_action(),
        Some(action_id),
        "während des Wartens muss der Indikator auf genau diese Aktion zeigen"
    );
    assert!(
        fixture.confirmations.contains(&action_id),
        "während des Wartens muss der Registry-Eintrag existieren"
    );

    fixture
        .confirmations
        .resolve(&action_id, ActionUserDecision::Approve)
        .expect("die Genehmigung muss den Wartenden erreichen");

    let executed = with_timeout("der Vorschlag wurde nie fertig", task)
        .await
        .expect("die Task darf nicht panicken");
    assert!(executed, "die genehmigte Aktion muss ausgeführt werden");
    assert_eq!(fixture.event_count("chat-action-result"), 1);
    assert_eq!(fixture.pending_action(), None);
}

/// T10 (A1.3, Regression): Eine zweite Auflösung derselben `action_id`
/// führt nichts ein zweites Mal aus.
#[tokio::test]
async fn test_a_second_resolution_of_the_same_action_executes_nothing_again() {
    let fixture = Fixture::with_session(session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default().with_response(TEST_COMMAND, output("weg")),
    ));
    let task = fixture.spawn_proposal();
    let action_id = fixture.await_pending_indicator().await;

    fixture
        .confirmations
        .resolve(&action_id, ActionUserDecision::Approve)
        .expect("die erste Genehmigung muss durchgehen");
    let executed = with_timeout("der Vorschlag wurde nie fertig", task)
        .await
        .expect("die Task darf nicht panicken");
    assert!(executed);

    assert!(
        fixture
            .confirmations
            .resolve(&action_id, ActionUserDecision::Approve)
            .is_err(),
        "die zweite Auflösung derselben action_id muss fehlschlagen"
    );
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        fixture.event_count("chat-action-result"),
        1,
        "genau eine Ausführung, auch nach einer zweiten Auflösung"
    );
    assert_eq!(fixture.pending_action(), None);
}

// --- Issue #66: Schließen lehnt die wartende Bestätigung JEDER Sitzung ab ---

/// Ein Nutzer-Vorschlag, der auf `Confirm` wartet, dessen Transport jeden
/// ausgeführten Befehl mitschreibt.
fn fixture_recording_execution() -> (Fixture, Arc<std::sync::Mutex<Vec<String>>>) {
    let transport = MockSshTransport::default().with_response(TEST_COMMAND, output(""));
    let executed = transport.executed_handle();
    let fixture = Fixture::with_session(test_session(vec![AiEvent::Done], transport));
    (fixture, executed)
}

/// Wie `app_shell::commands::disconnect`: erst aus dem `SessionManager`,
/// dann der Tauri-freie Schließ-Schritt — ohne jeden
/// `respond_to_action`-Aufruf aus dem Frontend.
fn close_like_disconnect(
    fixture: &Fixture,
    manager: &SessionManager,
    mcp_sessions: &crate::mcp_sessions::McpSessionRegistry,
) {
    let removed = manager.remove(fixture.session_id);
    crate::session::reject_pending_confirmation_on_close(
        fixture.session_id,
        removed.as_deref(),
        mcp_sessions,
        fixture.confirmations.as_ref(),
    );
}

/// Issue #66, AC 1 + AC 5 (Regression, gegen den Stand vor dem Fix rot:
/// dort lehnte `disconnect` nur MCP-Sitzungen ab, die Aktion blieb bis zum
/// Bestätigungs-Timeout offen und der Test lief in die Zeitgrenze):
/// Schließen einer **Nutzer**-Sitzung mit wartender Bestätigung, ohne
/// Frontend-Ablehnung, lehnt sie im Backend ab. Nichts läuft auf dem
/// Transport, und die Ablehnung wird genau wie ein Klick auf "Ablehnen"
/// verbucht (`RejectionReason::User` im Verlauf, Folgerunde angefordert).
#[tokio::test]
async fn test_closing_a_user_session_rejects_its_pending_confirmation_in_the_backend() {
    let (fixture, executed) = fixture_recording_execution();
    let manager = fixture.manager();
    let mcp_sessions = crate::mcp_sessions::McpSessionRegistry::new();
    let task = fixture.spawn_proposal();
    let action_id = fixture.await_pending_indicator().await;
    assert!(!mcp_sessions.is_mcp_session(fixture.session_id));

    close_like_disconnect(&fixture, &manager, &mcp_sessions);

    let needs_follow_up =
        with_timeout("die Bestätigung wurde beim Schließen nicht abgelehnt", task)
            .await
            .expect("die Task endet regulär");
    assert!(
        needs_follow_up,
        "eine Ablehnung fordert wie ein Klick auf \"Ablehnen\" die Folgerunde an"
    );
    assert!(
        executed.lock().unwrap().is_empty(),
        "Aktion lief trotz Schließen"
    );
    assert_eq!(fixture.event_count("chat-action-result"), 0);
    assert_eq!(fixture.pending_action(), None);
    assert!(!fixture.confirmations.contains(&action_id));
    assert!(
        fixture
            .confirmations
            .resolve(&action_id, ActionUserDecision::Approve)
            .is_err(),
        "nach dem Schließen darf sich die Aktion nicht mehr genehmigen lassen"
    );

    let history = fixture.session.context.lock().await.history.clone();
    assert!(
        history.iter().any(|m| matches!(
            &m.content,
            ssh_manager_core::ai::MessageContent::ActionRejected {
                reason: ssh_manager_core::ai::RejectionReason::User,
                ..
            }
        )),
        "Ablehnung fehlt im Verlauf (oder nicht als Nutzer-Ablehnung): {history:?}"
    );
}

/// Issue #66, AC 3: War die Bestätigung schon aufgelöst (hier: Klick auf
/// "Ablehnen" aus dem Frontend, wie `requestCloseTab` ihn weiter sendet),
/// ist das Schließen ein No-op ohne Fehler — und verbucht die Ablehnung
/// nicht doppelt.
#[tokio::test]
async fn test_closing_after_the_confirmation_was_settled_is_a_no_op() {
    let (fixture, executed) = fixture_recording_execution();
    let manager = fixture.manager();
    let mcp_sessions = crate::mcp_sessions::McpSessionRegistry::new();
    let task = fixture.spawn_proposal();
    let action_id = fixture.await_pending_indicator().await;

    fixture
        .confirmations
        .resolve(&action_id, ActionUserDecision::Deny)
        .expect("die Frontend-Ablehnung trifft die wartende Bestätigung");
    // Absichtlich VOR dem Ende der Task: Der Indikator kann noch stehen,
    // während der Registry-Eintrag schon weg ist.
    assert!(!fixture
        .session
        .reject_pending_confirmation(fixture.confirmations.as_ref()));
    close_like_disconnect(&fixture, &manager, &mcp_sessions);

    with_timeout("die Task endete nicht", task)
        .await
        .expect("die Task endet regulär");
    assert!(executed.lock().unwrap().is_empty());
    let history = fixture.session.context.lock().await.history.clone();
    let rejections = history
        .iter()
        .filter(|m| {
            matches!(
                m.content,
                ssh_manager_core::ai::MessageContent::ActionRejected { .. }
            )
        })
        .count();
    assert_eq!(rejections, 1, "Ablehnung doppelt verbucht: {history:?}");
}

/// Issue #66, AC 3: Ohne wartende Aktion ist das Schließen ein No-op —
/// auch ein bereits registrierter, fremder Eintrag bleibt unberührt.
#[test]
fn test_closing_a_session_without_pending_action_touches_nothing() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let mcp_sessions = crate::mcp_sessions::McpSessionRegistry::new();
    let other_action: ActionId = uuid::Uuid::new_v4();
    let _rx = fixture.confirmations.register(other_action);

    assert!(!fixture
        .session
        .reject_pending_confirmation(fixture.confirmations.as_ref()));
    close_like_disconnect(&fixture, &manager, &mcp_sessions);

    assert!(fixture.confirmations.contains(&other_action));
}

/// Issue #66, AC 4: Stand die Sitzung nicht (mehr) im `SessionManager`
/// (Schließen während des Host-Key-Dialogs), bleibt alles wie bisher — für
/// eine Nutzer-Sitzung passiert nichts, eine MCP-Sitzung wird ausgetragen.
#[test]
fn test_closing_a_session_missing_from_the_manager_behaves_as_before() {
    let confirmations = ConfirmationRegistry::new();
    let mcp_sessions = crate::mcp_sessions::McpSessionRegistry::new();
    let pending: ActionId = uuid::Uuid::new_v4();
    let _rx = confirmations.register(pending);

    let user_session_id: SessionId = uuid::Uuid::new_v4();
    crate::session::reject_pending_confirmation_on_close(
        user_session_id,
        None,
        &mcp_sessions,
        &confirmations,
    );
    assert!(confirmations.contains(&pending));

    let key = crate::mcp_sessions::McpSessionKey::new(
        ssh_manager_core::shared::ServerId::new(),
        Some("Claude Code"),
    );
    let mcp_session_id: SessionId = uuid::Uuid::new_v4();
    mcp_sessions.register(&key, mcp_session_id).unwrap();
    crate::session::reject_pending_confirmation_on_close(
        mcp_session_id,
        None,
        &mcp_sessions,
        &confirmations,
    );
    assert!(!mcp_sessions.is_mcp_session(mcp_session_id));
    assert!(confirmations.contains(&pending));
}

// ---------------------------------------------------------------------------
// Issue #108 / Spec 0104 §3 — höchstens eine wartende Bestätigung je
// MCP-Sitzung.
// ---------------------------------------------------------------------------

impl Fixture {
    fn spawn_mcp_proposal(&self) -> tokio::task::JoinHandle<bool> {
        let session = self.session.clone();
        let profile_store = self.profile_store.clone();
        let confirmations = self.confirmations.clone();
        let emitter = self.emitter.clone();
        let session_id = self.session_id;
        tokio::spawn(async move {
            super::handle_mcp_action_proposed(
                &session,
                session_id,
                AiAction::SuggestCommand {
                    command: TEST_COMMAND.to_string(),
                },
                emitter.as_ref(),
                profile_store.as_ref(),
                confirmations.as_ref(),
                Some("client".to_string()),
            )
            .await
        })
    }
}

#[tokio::test]
async fn test_issue_108_second_mcp_proposal_is_rejected_while_one_waits() {
    let fixture = Fixture::new();
    let first = fixture.spawn_mcp_proposal();
    let first_id = fixture.await_pending_indicator().await;

    let second = fixture.spawn_mcp_proposal();
    let second_done = with_timeout("der zweite Vorschlag wurde nicht sofort abgewiesen", second)
        .await
        .expect("Task ohne Panic");
    assert!(second_done);
    assert_eq!(fixture.event_count("chat-error"), 1);
    assert_eq!(
        fixture.event_count("chat-action-proposed"),
        1,
        "der abgewiesene Vorschlag darf keine zweite Karte erzeugen"
    );
    // Die erste Bestätigung wartet unverändert weiter.
    assert_eq!(fixture.pending_action(), Some(first_id));
    assert!(fixture.confirmations.contains(&first_id));

    fixture
        .confirmations
        .resolve(&first_id, ActionUserDecision::Deny)
        .expect("erste Bestätigung ist noch offen");
    with_timeout("die erste Task endete nicht", first)
        .await
        .expect("Task ohne Panic");
    assert_eq!(fixture.pending_action(), None);
}

#[tokio::test]
async fn test_issue_108_a_new_mcp_proposal_is_accepted_after_the_wait_ended() {
    let fixture = Fixture::new();
    let first = fixture.spawn_mcp_proposal();
    let first_id = fixture.await_pending_indicator().await;
    first.abort();
    let _ = with_timeout("die abgebrochene Task endete nicht", first).await;
    assert_eq!(fixture.pending_action(), None);

    let second = fixture.spawn_mcp_proposal();
    let second_id = with_timeout("der neue Vorschlag wartet nicht", async {
        loop {
            if let Some(id) = fixture.pending_action() {
                return id;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_ne!(first_id, second_id);
    assert_eq!(fixture.event_count("chat-error"), 0);
    fixture
        .confirmations
        .resolve(&second_id, ActionUserDecision::Deny)
        .expect("offen");
    with_timeout("zweite Task endete nicht", second)
        .await
        .expect("Task ohne Panic");
}

#[tokio::test]
async fn test_issue_108_closing_the_session_rejects_the_waiting_mcp_confirmation() {
    let fixture = Fixture::new();
    let task = fixture.spawn_mcp_proposal();
    fixture.await_pending_indicator().await;
    let manager = fixture.manager();
    assert!(fixture.snapshot_has_pending_action(&manager));

    assert!(fixture
        .session
        .reject_pending_confirmation(fixture.confirmations.as_ref()));
    with_timeout("die wartende Task endete nicht", task)
        .await
        .expect("Task ohne Panic");
    assert!(!fixture.snapshot_has_pending_action(&manager));
}
