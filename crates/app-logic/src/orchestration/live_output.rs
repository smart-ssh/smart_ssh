//! Issue #325 (Spec 0106): forwards the live output of a running command to
//! the frontend — redacted (`ssh_manager_core::ai::LiveOutputRedactor`) and
//! throttled to at most one `chat-action-output` event per
//! [`LIVE_OUTPUT_INTERVAL`] and action. Display only: nothing here reaches
//! the chat history, the ledger or the AI.

use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::MissedTickBehavior;

use ssh_manager_core::ai::{LiveOutputRedactor, OutputRedactor};
use ssh_manager_core::ssh::{ExecOutputChunk, OutputStream};

use crate::events::{emit_chat_action_output, EventEmitter};
use crate::state::{ActionId, SessionId};

/// At most ten live updates per second and action.
pub(crate) const LIVE_OUTPUT_INTERVAL: Duration = Duration::from_millis(100);

/// Runs until the transport drops its end of the channel (the command has
/// ended, was cancelled or failed). Output still held back at that point is
/// not flushed: the final `chat-action-result` replaces the live view.
pub(crate) async fn forward_live_output(
    mut chunks: UnboundedReceiver<ExecOutputChunk>,
    redactor: &dyn OutputRedactor,
    emitter: &dyn EventEmitter,
    session_id: SessionId,
    action_id: ActionId,
) {
    let mut stdout = LiveOutputRedactor::new();
    let mut stderr = LiveOutputRedactor::new();
    let mut truncated = false;
    let mut truncated_sent = false;
    let mut dirty = false;
    let mut ticker = tokio::time::interval(LIVE_OUTPUT_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // The first tick completes immediately; consume it so the first event
    // comes one interval after the start at the earliest.
    ticker.tick().await;
    loop {
        tokio::select! {
            chunk = chunks.recv() => match chunk {
                None => break,
                Some(ExecOutputChunk::Data { stream, bytes }) => {
                    match stream {
                        OutputStream::Stdout => stdout.push(&bytes),
                        OutputStream::Stderr => stderr.push(&bytes),
                    }
                    dirty = true;
                }
                Some(ExecOutputChunk::Truncated) => {
                    truncated = true;
                    dirty = true;
                }
            },
            _ = ticker.tick(), if dirty => {
                dirty = false;
                let out = stdout.release(redactor);
                let err = stderr.release(redactor);
                let newly_truncated = truncated && !truncated_sent;
                if !out.is_empty() || !err.is_empty() || newly_truncated {
                    truncated_sent = truncated;
                    emit_chat_action_output(emitter, session_id, action_id, &out, &err, truncated);
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::events::TestEmitter;
    use ssh_manager_core::ai::DefaultOutputRedactor;
    use uuid::Uuid;

    fn output_events(emitter: &TestEmitter) -> Vec<serde_json::Value> {
        emitter
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == "chat-action-output")
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    fn joined(events: &[serde_json::Value], field: &str) -> String {
        events
            .iter()
            .map(|e| e[field].as_str().unwrap().to_string())
            .collect()
    }

    /// AC "fast producer": `yes | head -c 1500000` delivered as many small
    /// chunks within one (virtual) second yields at most about ten events.
    #[tokio::test(start_paused = true)]
    async fn test_fast_producer_is_throttled_to_ten_events_per_second() {
        let (sink, chunks) = tokio::sync::mpsc::unbounded_channel();
        let emitter = TestEmitter::default();
        let redactor = DefaultOutputRedactor::new();
        let session_id = Uuid::new_v4();
        let action_id = Uuid::new_v4();

        let producer = async move {
            // 1_500_000 bytes of "y\n" in 1000 chunks over one second.
            let chunk = b"y\n".repeat(750);
            for _ in 0..1000 {
                sink.send(ExecOutputChunk::Data {
                    stream: OutputStream::Stdout,
                    bytes: chunk.clone(),
                })
                .unwrap();
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        };
        let started = tokio::time::Instant::now();
        tokio::join!(
            producer,
            forward_live_output(chunks, &redactor, &emitter, session_id, action_id)
        );
        let elapsed = started.elapsed();

        let events = output_events(&emitter);
        let max_events = (elapsed.as_millis() / LIVE_OUTPUT_INTERVAL.as_millis()) as usize + 1;
        assert!(
            !events.is_empty() && events.len() <= max_events && events.len() <= 11,
            "{} events in {elapsed:?}",
            events.len()
        );
        assert!(events
            .iter()
            .all(|e| e["actionId"] == serde_json::json!(action_id)
                && e["sessionId"] == serde_json::json!(session_id)));
        // Everything but the held-back last line arrived live.
        assert_eq!(joined(&events, "stdout").len(), 1_500_000 - 2);
    }

    /// AC "lines appear one after another": each line is sent once the next
    /// one followed, in its own event when they arrive a second apart.
    #[tokio::test(start_paused = true)]
    async fn test_slow_lines_appear_one_after_another() {
        let (sink, chunks) = tokio::sync::mpsc::unbounded_channel();
        let emitter = TestEmitter::default();
        let redactor = DefaultOutputRedactor::new();

        let producer = async move {
            for i in 1..=5 {
                sink.send(ExecOutputChunk::Data {
                    stream: OutputStream::Stdout,
                    bytes: format!("{i}\n").into_bytes(),
                })
                .unwrap();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        };
        tokio::join!(
            producer,
            forward_live_output(chunks, &redactor, &emitter, Uuid::new_v4(), Uuid::new_v4())
        );

        let stdout: Vec<String> = output_events(&emitter)
            .iter()
            .map(|e| e["stdout"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(stdout, vec!["1\n", "2\n", "3\n", "4\n"]);
    }

    /// AC "stderr appears live and as stderr".
    #[tokio::test(start_paused = true)]
    async fn test_stderr_is_forwarded_as_stderr() {
        let (sink, chunks) = tokio::sync::mpsc::unbounded_channel();
        let emitter = TestEmitter::default();
        let redactor = DefaultOutputRedactor::new();
        sink.send(ExecOutputChunk::Data {
            stream: OutputStream::Stderr,
            bytes: b"warning one\nwarning two\n".to_vec(),
        })
        .unwrap();
        sink.send(ExecOutputChunk::Data {
            stream: OutputStream::Stdout,
            bytes: b"out one\nout two\n".to_vec(),
        })
        .unwrap();
        let producer = async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            drop(sink);
        };
        tokio::join!(
            producer,
            forward_live_output(chunks, &redactor, &emitter, Uuid::new_v4(), Uuid::new_v4())
        );

        let events = output_events(&emitter);
        assert_eq!(joined(&events, "stderr"), "warning one\n");
        assert_eq!(joined(&events, "stdout"), "out one\n");
    }

    /// AC "secret masked in the live view": a credential the redactor masks
    /// in the final result never appears in any live event, also when the
    /// chunk boundary splits it.
    #[tokio::test(start_paused = true)]
    async fn test_secret_is_masked_in_the_live_view() {
        const SECRET: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
        let (sink, chunks) = tokio::sync::mpsc::unbounded_channel();
        let emitter = TestEmitter::default();
        let redactor = DefaultOutputRedactor::new();
        let raw = format!("cloning\ntoken {SECRET} in use\ndone\nend\n");
        let producer = async move {
            for part in raw.as_bytes().chunks(5) {
                sink.send(ExecOutputChunk::Data {
                    stream: OutputStream::Stdout,
                    bytes: part.to_vec(),
                })
                .unwrap();
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        };
        tokio::join!(
            producer,
            forward_live_output(chunks, &redactor, &emitter, Uuid::new_v4(), Uuid::new_v4())
        );

        let shown = joined(&output_events(&emitter), "stdout");
        assert!(shown.contains("[REDACTED]"), "{shown:?}");
        assert!(!shown.contains("ghp_abc"), "{shown:?}");
        assert!(!shown.contains("0123456789"), "{shown:?}");
    }

    /// AC "truncation notice in the live view": `Truncated` is reported once.
    #[tokio::test(start_paused = true)]
    async fn test_truncation_is_reported_once() {
        let (sink, chunks) = tokio::sync::mpsc::unbounded_channel();
        let emitter = TestEmitter::default();
        let redactor = DefaultOutputRedactor::new();
        sink.send(ExecOutputChunk::Data {
            stream: OutputStream::Stdout,
            bytes: b"a\nb\n".to_vec(),
        })
        .unwrap();
        sink.send(ExecOutputChunk::Truncated).unwrap();
        let producer = async move {
            tokio::time::sleep(Duration::from_millis(350)).await;
            drop(sink);
        };
        tokio::join!(
            producer,
            forward_live_output(chunks, &redactor, &emitter, Uuid::new_v4(), Uuid::new_v4())
        );

        let events = output_events(&emitter);
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["truncated"], serde_json::json!(true));
        assert_eq!(events[0]["stdout"], serde_json::json!("a\n"));
    }
}
