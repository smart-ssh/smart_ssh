//! Issue #325 (Spec 0106): live output of a running command through the
//! whole execution path — live events before the final result, final result
//! unchanged, cancel during streaming, two sessions side by side.

use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::ssh::{ExecOutputChunk, OutputStream};

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

fn stdout_chunk(text: &str) -> ExecOutputChunk {
    ExecOutputChunk::Data {
        stream: OutputStream::Stdout,
        bytes: text.as_bytes().to_vec(),
    }
}

fn event_names(emitter: &TestEmitter) -> Vec<String> {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

fn live_stdout(emitter: &TestEmitter, action_id: ActionId) -> String {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, payload)| {
            name == "chat-action-output" && payload["actionId"] == serde_json::json!(action_id)
        })
        .map(|(_, payload)| payload["stdout"].as_str().unwrap().to_string())
        .collect()
}

/// Live events come while the command runs and always before
/// `chat-action-result`; the final result, the history entry and what
/// goes to the AI are exactly what the transport returned.
#[tokio::test(start_paused = true)]
async fn test_live_output_precedes_an_unchanged_final_result() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default()
            .with_streamed_output(
                "count",
                vec![
                    stdout_chunk("1\n"),
                    stdout_chunk("2\n"),
                    stdout_chunk("3\n"),
                ],
            )
            .with_response("count", output("1\n2\n3\n")),
    );
    let emitter = TestEmitter::default();
    let action_id = Uuid::new_v4();

    let executed = execute_suggested_command(
        &session,
        Uuid::new_v4(),
        action_id,
        "count".to_string(),
        &emitter,
        true,
        LedgerSource::Ai,
    )
    .await;
    assert!(executed);

    let names = event_names(&emitter);
    let result_at = names
        .iter()
        .position(|n| n == "chat-action-result")
        .expect("chat-action-result");
    let output_positions: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, n)| *n == "chat-action-output")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(output_positions.len(), 2, "{names:?}");
    assert!(output_positions.iter().all(|i| *i < result_at));
    assert_eq!(live_stdout(&emitter, action_id), "1\n2\n");

    let events = emitter.events.lock().unwrap().clone();
    assert_eq!(
        events[result_at].1["result"]["stdout"],
        serde_json::json!("1\n2\n3\n")
    );
    let history = session.context.lock().await.history.clone();
    assert_eq!(history.len(), 1);
    let MessageContent::CommandResult {
        output, cancelled, ..
    } = &history[0].content
    else {
        panic!("expected CommandResult, got {:?}", history[0].content);
    };
    assert_eq!(output.stdout, b"1\n2\n3\n");
    assert!(!cancelled);
}

/// Cancel while output is streaming: the command ends, the AI gets the
/// partial output with the cancelled flag — as without streaming.
#[tokio::test(start_paused = true)]
async fn test_cancel_during_streaming_keeps_partial_output_for_the_ai() {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default()
            .with_streamed_output(
                "journalctl -f",
                vec![stdout_chunk("line a\n"), stdout_chunk("line b\n")],
            )
            .with_never_completing("journalctl -f"),
    );
    let emitter = TestEmitter::default();
    let action_id = Uuid::new_v4();

    let exec_future = execute_suggested_command(
        &session,
        Uuid::new_v4(),
        action_id,
        "journalctl -f".to_string(),
        &emitter,
        true,
        LedgerSource::Ai,
    );
    let cancel_future = async {
        // Cancel only once live output was shown.
        while live_stdout(&emitter, action_id).is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        session
            .running_command_cancellations
            .resolve(&action_id, ())
            .expect("a waiting cancel registration");
    };
    let (executed, ()) = tokio::join!(exec_future, cancel_future);
    assert!(executed);

    assert_eq!(live_stdout(&emitter, action_id), "line a\n");
    let events = emitter.events.lock().unwrap().clone();
    let (_, result) = events
        .iter()
        .find(|(name, _)| name == "chat-action-result")
        .expect("chat-action-result");
    assert_eq!(result["result"]["cancelled"], serde_json::json!(true));
    let history = session.context.lock().await.history.clone();
    let MessageContent::CommandResult {
        cancelled, output, ..
    } = &history[0].content
    else {
        panic!("expected CommandResult, got {:?}", history[0].content);
    };
    assert!(cancelled);
    assert_eq!(output.stdout, b"partial output before cancel");
}

/// Two sessions running commands at the same time: every live event
/// carries its own session and action, and the text of one never shows up
/// in the other.
#[tokio::test(start_paused = true)]
async fn test_two_sessions_stream_only_their_own_output() {
    let session_a = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default()
            .with_streamed_output(
                "run",
                vec![stdout_chunk("alpha 1\n"), stdout_chunk("alpha 2\n")],
            )
            .with_response("run", output("alpha 1\nalpha 2\n")),
    );
    let session_b = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default()
            .with_streamed_output(
                "run",
                vec![stdout_chunk("beta 1\n"), stdout_chunk("beta 2\n")],
            )
            .with_response("run", output("beta 1\nbeta 2\n")),
    );
    let emitter = TestEmitter::default();
    let (session_id_a, action_a) = (Uuid::new_v4(), Uuid::new_v4());
    let (session_id_b, action_b) = (Uuid::new_v4(), Uuid::new_v4());

    tokio::join!(
        execute_suggested_command(
            &session_a,
            session_id_a,
            action_a,
            "run".to_string(),
            &emitter,
            true,
            LedgerSource::Ai,
        ),
        execute_suggested_command(
            &session_b,
            session_id_b,
            action_b,
            "run".to_string(),
            &emitter,
            true,
            LedgerSource::Ai,
        ),
    );

    let events = emitter.events.lock().unwrap().clone();
    for (_, payload) in events.iter().filter(|(n, _)| n == "chat-action-output") {
        let text = payload["stdout"].as_str().unwrap();
        if payload["actionId"] == serde_json::json!(action_a) {
            assert_eq!(payload["sessionId"], serde_json::json!(session_id_a));
            assert!(!text.contains("beta"), "{text:?}");
        } else {
            assert_eq!(payload["actionId"], serde_json::json!(action_b));
            assert_eq!(payload["sessionId"], serde_json::json!(session_id_b));
            assert!(!text.contains("alpha"), "{text:?}");
        }
    }
    assert_eq!(live_stdout(&emitter, action_a), "alpha 1\n");
    assert_eq!(live_stdout(&emitter, action_b), "beta 1\n");
}

fn waiting_hint_shown(emitter: &TestEmitter) -> bool {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|(name, payload)| name == "chat-action-waiting-input" && payload["waiting"] == true)
}

/// Issue #327: stopping while the "probably waiting for input" hint is
/// shown marks the history entry, so the AI note asks for a non-interactive
/// variant. Stopping a silent command (no prompt as last line) does not.
async fn stop_after(command: &str, chunk: &str, wait_for_hint: bool) -> (bool, bool) {
    let session = session_with_ai_provider(
        MockAiProvider::new(vec![AiEvent::Done]),
        MockSshTransport::default()
            .with_streamed_output(command, vec![stdout_chunk(chunk)])
            .with_never_completing(command),
    );
    let emitter = TestEmitter::default();
    let action_id = Uuid::new_v4();
    let exec_future = execute_suggested_command(
        &session,
        Uuid::new_v4(),
        action_id,
        command.to_string(),
        &emitter,
        true,
        LedgerSource::Ai,
    );
    let cancel_future = async {
        if wait_for_hint {
            // Fail instead of hanging if the hint never appears.
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                while !waiting_hint_shown(&emitter) {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .expect("the waiting-for-input hint was never emitted");
        } else {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        session
            .running_command_cancellations
            .resolve(&action_id, ())
            .expect("a waiting cancel registration");
    };
    let (executed, ()) = tokio::join!(exec_future, cancel_future);
    assert!(executed);
    let history = session.context.lock().await.history.clone();
    let MessageContent::CommandResult {
        cancelled,
        waiting_for_input,
        ..
    } = &history[0].content
    else {
        panic!("expected CommandResult, got {:?}", history[0].content);
    };
    (*cancelled, *waiting_for_input)
}

#[tokio::test(start_paused = true)]
async fn test_stop_while_waiting_for_input_is_marked_for_the_ai() {
    let (cancelled, waiting) =
        stop_after("apt install x", "Do you want to continue? [Y/n] ", true).await;
    assert!(cancelled);
    assert!(waiting);
}

#[tokio::test(start_paused = true)]
async fn test_stop_of_a_command_without_prompt_is_not_marked() {
    let (cancelled, waiting) = stop_after("tail -f log", "working\n", false).await;
    assert!(cancelled);
    assert!(!waiting);
}
