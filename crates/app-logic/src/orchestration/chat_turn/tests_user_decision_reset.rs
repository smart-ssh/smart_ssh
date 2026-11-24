//! Issue #1296 / Spec 0021, Abschnitt 4: the auto-continuation cap counts
//! only rounds since the last genuine user decision.

use std::sync::Mutex as StdMutex;

use async_trait::async_trait;
use uuid::Uuid;

use ssh_manager_core::ai::AiProvider;
use ssh_manager_core::filter::{
    EffectiveScope, FilterEngine, Pattern, PolicyRules, PolicyStore, Rule, RuleAction, RuleId,
    RuleOrigin, Scope,
};

use crate::dto::ActionUserDecision;
use crate::events::TestEmitter;
use crate::state::ActionId;

use super::super::test_support::*;
use super::*;

/// Proposes the next scripted command per round, then finishes with text.
struct ScriptProvider {
    script: Vec<String>,
    next: StdMutex<usize>,
}

impl AiProvider for ScriptProvider {
    fn send(
        &self,
        _context: RedactedContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = AiEvent> + Send>> {
        let mut next = self.next.lock().unwrap();
        let events = match self.script.get(*next) {
            Some(command) => vec![
                AiEvent::ActionProposed(AiAction::SuggestCommand {
                    command: command.clone(),
                }),
                AiEvent::Done,
            ],
            None => vec![AiEvent::TextDelta("done".to_string()), AiEvent::Done],
        };
        *next += 1;
        Box::pin(futures::stream::iter(events))
    }
}

struct PrefixPolicyStore;
#[async_trait]
impl PolicyStore for PrefixPolicyStore {
    async fn rules_for(&self, _scope: &EffectiveScope) -> PolicyRules {
        let rule = |id: &str, glob: &str, action| Rule {
            id: RuleId(id.to_string()),
            pattern: Pattern::Glob(glob.to_string()),
            action,
            scope: Scope::Global,
            priority: 0,
            origin: RuleOrigin::User,
        };
        PolicyRules::complete(vec![
            rule("allow-echo", "echo*", RuleAction::Allow),
            rule("confirm", "confirm*", RuleAction::Confirm),
            rule("deny", "deny*", RuleAction::Deny),
        ])
    }
}

/// Runs a turn over `script`; every confirmation prompt is answered with
/// the next entry of `decisions`. Returns the emitted events and the
/// number of proposals.
async fn run_script(
    script: Vec<String>,
    decisions: Vec<ActionUserDecision>,
) -> (TestEmitter, usize) {
    run_script_with(script, decisions.into_iter().map(Some).collect(), false).await
}

/// Like `run_script`; an answer of `None` leaves the prompt unanswered
/// (it times out) or, with `drop_sender`, drops the pending sender.
async fn run_script_with(
    script: Vec<String>,
    decisions: Vec<Option<ActionUserDecision>>,
    drop_sender: bool,
) -> (TestEmitter, usize) {
    let provider = ScriptProvider {
        script,
        next: StdMutex::new(0),
    };
    let mut session = session_with_ai_provider(provider, MockSshTransport::default());
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(PrefixPolicyStore)));
    let emitter = TestEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();

    let turn = async {
        run_chat_turn(
            &session,
            Uuid::new_v4(),
            &emitter,
            &profile_store,
            &confirmations,
        )
        .await;
        true
    };
    let responder = async {
        let mut answered = 0usize;
        let mut decisions = decisions.into_iter();
        loop {
            let pending: Option<ActionId> = {
                let events = emitter.events.lock().unwrap();
                events
                    .iter()
                    .filter(|(name, payload)| {
                        name == "chat-action-proposed"
                            && payload
                                .get("decision")
                                .and_then(|d| d.get("Confirm"))
                                .is_some()
                    })
                    .nth(answered)
                    .map(|(_, p)| p["actionId"].as_str().unwrap().parse().unwrap())
            };
            if let Some(id) = pending {
                answered += 1;
                let decision = decisions.next().expect("more prompts than decisions");
                match decision {
                    Some(d) => confirmations.resolve(&id, d).unwrap(),
                    // Re-registering the key drops the waiting sender.
                    None if drop_sender => drop(confirmations.register(id)),
                    None => {}
                }
            }
            // Sleeping (not yielding) lets the paused clock auto-advance
            // through the request pacing between rounds.
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    };
    // Virtual-time bound: a hang fails instead of blocking forever.
    tokio::time::timeout(std::time::Duration::from_secs(360_000), async {
        tokio::select! {
            done = turn => done,
            () = responder => unreachable!(),
        }
    })
    .await
    .expect("turn did not end");
    let proposed = emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(n, _)| n == "chat-action-proposed")
        .count();
    (emitter, proposed)
}

fn limit_reached(emitter: &TestEmitter) -> bool {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|(n, _)| n == "chat-auto-continuation-limit-reached")
}

fn cmds(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{prefix} {i}")).collect()
}

#[tokio::test(start_paused = true)]
async fn test_unprompted_rounds_still_stop_at_the_cap() {
    let (emitter, proposed) = run_script(cmds("echo", 25), vec![]).await;
    assert_eq!(proposed, MAX_AUTO_FOLLOWUP_ROUNDS);
    assert!(limit_reached(&emitter));
}

#[tokio::test(start_paused = true)]
async fn test_approval_every_nine_rounds_runs_past_the_cap() {
    // Rounds 9, 18 are confirmations; 25 rounds total.
    let mut script = cmds("echo", 25);
    script[8] = "confirm a".into();
    script[17] = "confirm b".into();
    let (emitter, proposed) = run_script(
        script,
        vec![ActionUserDecision::Approve, ActionUserDecision::Approve],
    )
    .await;
    assert_eq!(proposed, 25);
    assert!(!limit_reached(&emitter));
}

#[tokio::test(start_paused = true)]
async fn test_exactly_ten_rounds_after_an_approval_stop() {
    let mut script = cmds("echo", 40);
    script[2] = "confirm a".into();
    let (emitter, proposed) = run_script(script, vec![ActionUserDecision::Approve]).await;
    // Rounds 1-3 (approval in 3), then 10 more rounds.
    assert_eq!(proposed, 3 + MAX_AUTO_FOLLOWUP_ROUNDS);
    assert!(limit_reached(&emitter));
}

async fn assert_decision_resets(decision: ActionUserDecision) {
    let mut script = cmds("echo", 40);
    script[4] = "confirm a".into();
    let (emitter, proposed) = run_script(script, vec![decision]).await;
    assert_eq!(proposed, 5 + MAX_AUTO_FOLLOWUP_ROUNDS);
    assert!(limit_reached(&emitter));
}

#[tokio::test(start_paused = true)]
async fn test_deny_resets_the_counter() {
    assert_decision_resets(ActionUserDecision::Deny).await;
}

#[tokio::test(start_paused = true)]
async fn test_edit_then_approve_resets_the_counter() {
    assert_decision_resets(ActionUserDecision::EditThenApprove {
        command: "echo edited".to_string(),
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn test_deny_reconnect_does_not_reset_the_counter() {
    let mut script = cmds("echo", 40);
    script[4] = "confirm a".into();
    let (emitter, proposed) = run_script(script, vec![ActionUserDecision::DenyReconnect]).await;
    assert_eq!(proposed, MAX_AUTO_FOLLOWUP_ROUNDS);
    assert!(limit_reached(&emitter));
}

#[tokio::test(start_paused = true)]
async fn test_blocked_actions_do_not_reset_the_counter() {
    let (emitter, proposed) = run_script(cmds("deny", 40), vec![]).await;
    assert_eq!(proposed, MAX_AUTO_FOLLOWUP_ROUNDS);
    assert!(limit_reached(&emitter));
}

#[tokio::test(start_paused = true)]
async fn test_queued_message_resets_the_counter() {
    let provider = ScriptProvider {
        script: cmds("echo", 40),
        next: StdMutex::new(0),
    };
    let mut session = session_with_ai_provider(provider, MockSshTransport::default());
    session.set_filter_engine_for_tests(Box::new(FilterEngine::new(PrefixPolicyStore)));
    // Taken at the boundary into round 2.
    session
        .conn
        .chat_turn
        .lock()
        .unwrap()
        .queued
        .push("keep going".to_string());
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
    let proposed = emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(n, _)| n == "chat-action-proposed")
        .count();
    // Round 1, then the reset at round 2 gives 10 rounds from there.
    assert_eq!(proposed, 1 + MAX_AUTO_FOLLOWUP_ROUNDS);
    assert!(limit_reached(&emitter));
}

#[tokio::test(start_paused = true)]
async fn test_confirmation_timeout_does_not_reset_the_counter() {
    let mut script = cmds("echo", 40);
    script[4] = "confirm a".into();
    // The prompt is never answered: it times out (RejectionReason::Timeout).
    let (emitter, proposed) = run_script_with(script, vec![None], false).await;
    assert_eq!(proposed, MAX_AUTO_FOLLOWUP_ROUNDS);
    assert!(limit_reached(&emitter));
}

#[tokio::test(start_paused = true)]
async fn test_dropped_confirmation_sender_does_not_reset_the_counter() {
    let mut script = cmds("echo", 40);
    script[4] = "confirm a".into();
    let (emitter, proposed) = run_script_with(script, vec![None], true).await;
    // A dropped sender counts as an earlier rejection and ends the chain
    // right after that round (it is not a user decision, so no reset and
    // no continuation): the 5th proposal is the last.
    assert_eq!(proposed, 5);
    assert!(!limit_reached(&emitter));
}
