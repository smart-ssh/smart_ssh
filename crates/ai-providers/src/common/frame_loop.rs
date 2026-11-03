//! The one SSE frame loop shared by all providers (issue #855).
//!
//! A provider only implements [`FrameHandler`]: how a frame is parsed and how
//! the stream is finalised. The loop owns everything else: the content
//! deadline (issue #694), transport errors, the inactivity timeout and the
//! abrupt end of stream.
//!
//! The withheld-tool-call rule (Spec 0065 / ADR 0056, security invariant 5)
//! lives in the handlers' `finalize`: the loop always calls it with
//! `abrupt = true` when the connection ends without the handler having
//! reported a terminal frame.

use std::collections::VecDeque;
use std::pin::Pin;

use futures::{Stream, StreamExt};
use ssh_manager_core::ai::{AiError, AiEvent};

use crate::error::{map_transport_error, timeout_error};
use crate::sse::{
    frame_is_content, sse_frame_stream, SseFrame, CONTENT_TIMEOUT, SSE_INACTIVITY_TIMEOUT,
};

pub(crate) type FrameStream = Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>>;

/// Provider-specific part of the frame loop.
pub(crate) trait FrameHandler {
    /// The provider's internal event type (wraps a public `AiEvent`).
    type Event: From<AiEvent> + Send + 'static;

    /// Feeds one frame; returns the events it produced. If the frame was
    /// terminal, the handler has finalised itself (for the non-abrupt case)
    /// and [`FrameHandler::is_finished`] returns `true` afterwards.
    fn on_frame(&mut self, frame: &SseFrame) -> Vec<Self::Event>;

    /// `true` once a terminal frame (or a terminal in-stream error) was seen.
    fn is_finished(&self) -> bool;

    /// Called when the connection ended without a terminal frame
    /// (`abrupt = true`). Must never release an incomplete tool call.
    fn finalize(&mut self, abrupt: bool) -> Vec<Self::Event>;

    /// Logs a transport/timeout error with the handler's secrets redacted.
    fn log_transport_error(&self, error: &AiError);
}

struct LoopState<H: FrameHandler> {
    handler: H,
    frames: FrameStream,
    /// Issue #694: restarted only by real content frames.
    content_deadline: tokio::time::Instant,
    pending: VecDeque<H::Event>,
    finished: bool,
}

impl<H: FrameHandler> LoopState<H> {
    fn push_error(&mut self, mapped: AiError) {
        self.handler.log_transport_error(&mapped);
        self.pending
            .push_back(H::Event::from(AiEvent::Error(mapped)));
        self.finished = true;
    }
}

/// Separate from [`event_stream_from_response`] so the timeout behaviour can
/// be tested with a synthetic `frames` stream, without an HTTP request.
pub(crate) fn process_frame_stream<H>(
    frames: FrameStream,
    handler: H,
) -> Pin<Box<dyn Stream<Item = H::Event> + Send>>
where
    H: FrameHandler + Send + 'static,
{
    let state = LoopState {
        handler,
        frames,
        content_deadline: tokio::time::Instant::now() + CONTENT_TIMEOUT,
        pending: VecDeque::new(),
        finished: false,
    };
    Box::pin(futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Some((event, state));
            }
            if state.finished {
                return None;
            }
            match tokio::time::timeout_at(state.content_deadline, state.frames.next()).await {
                Ok(Some(Ok(frame))) => {
                    if frame_is_content(&frame) {
                        state.content_deadline = tokio::time::Instant::now() + CONTENT_TIMEOUT;
                    }
                    let events = state.handler.on_frame(&frame);
                    state.pending.extend(events);
                    if state.handler.is_finished() {
                        state.finished = true;
                    }
                }
                Ok(Some(Err(err))) => state.push_error(map_transport_error(&err)),
                Ok(None) => {
                    // The connection ended without a terminal frame.
                    state.finished = true;
                    let events = state.handler.finalize(true);
                    state.pending.extend(events);
                }
                // See `SSE_INACTIVITY_TIMEOUT`: without this limit a hanging
                // request would block the chat turn forever, silently.
                Err(_elapsed) => state.push_error(timeout_error(SSE_INACTIVITY_TIMEOUT)),
            }
        }
    }))
}

pub(crate) fn event_stream_from_response<H>(
    response: reqwest::Response,
    handler: H,
) -> Pin<Box<dyn Stream<Item = H::Event> + Send>>
where
    H: FrameHandler + Send + 'static,
{
    process_frame_stream(Box::pin(sse_frame_stream(response)), handler)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::max_tokens_retry::RawEvent;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    struct Fake {
        finished: bool,
        finalize_calls: Arc<Mutex<Vec<bool>>>,
        logged: Arc<Mutex<usize>>,
    }

    impl FrameHandler for Fake {
        type Event = RawEvent;

        fn on_frame(&mut self, frame: &SseFrame) -> Vec<RawEvent> {
            if frame.data == "end" {
                self.finished = true;
                self.finalize_calls.lock().unwrap().push(false);
                return vec![RawEvent::Public(AiEvent::TextDelta("fin".into()))];
            }
            vec![RawEvent::Public(AiEvent::TextDelta(frame.data.clone()))]
        }
        fn is_finished(&self) -> bool {
            self.finished
        }
        fn finalize(&mut self, abrupt: bool) -> Vec<RawEvent> {
            self.finalize_calls.lock().unwrap().push(abrupt);
            Vec::new()
        }
        fn log_transport_error(&self, _error: &AiError) {
            *self.logged.lock().unwrap() += 1;
        }
    }

    fn frames_of(items: Vec<Result<SseFrame, reqwest::Error>>) -> FrameStream {
        Box::pin(futures::stream::iter(items))
    }

    fn frame(event: Option<&str>, data: &str) -> SseFrame {
        SseFrame {
            event: event.map(str::to_string),
            data: data.to_string(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_loop_content_timeout_yields_error_and_logs() {
        let fake = Fake::default();
        let logged = fake.logged.clone();
        let never: FrameStream = Box::pin(futures::stream::pending());
        let events: Vec<RawEvent> = process_frame_stream(never, fake).collect().await;
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RawEvent::Public(AiEvent::Error(_))));
        assert_eq!(*logged.lock().unwrap(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn test_loop_keep_alive_does_not_reset_deadline_but_content_does() {
        // Keep-alives every 5 s never restart the deadline: error after 120 s.
        let keep_alive = crate::sse::periodic_frames(None, "", Duration::from_secs(5));
        let events: Vec<RawEvent> = process_frame_stream(keep_alive, Fake::default())
            .collect()
            .await;
        assert!(matches!(
            events.last(),
            Some(RawEvent::Public(AiEvent::Error(_)))
        ));
        // Content deltas every 5 s keep the stream alive well past 120 s.
        let deltas = crate::sse::periodic_frames(Some("x"), "d", Duration::from_secs(5));
        let mut stream = process_frame_stream(deltas, Fake::default());
        let started = tokio::time::Instant::now();
        while started.elapsed() < Duration::from_secs(300) {
            let ev = stream.next().await.expect("stream ended");
            assert!(matches!(ev, RawEvent::Public(AiEvent::TextDelta(_))));
        }
    }

    #[tokio::test]
    async fn test_loop_abrupt_eof_calls_finalize_true() {
        let fake = Fake::default();
        let calls = fake.finalize_calls.clone();
        let frames = frames_of(vec![Ok(frame(None, "a"))]);
        let events: Vec<RawEvent> = process_frame_stream(frames, fake).collect().await;
        assert_eq!(events.len(), 1);
        assert_eq!(*calls.lock().unwrap(), vec![true]);
    }

    #[tokio::test]
    async fn test_loop_normal_end_calls_finalize_false_only() {
        let fake = Fake::default();
        let calls = fake.finalize_calls.clone();
        let frames = frames_of(vec![
            Ok(frame(None, "a")),
            Ok(frame(None, "end")),
            Ok(frame(None, "late")),
        ]);
        let events: Vec<RawEvent> = process_frame_stream(frames, fake).collect().await;
        // Frames after the terminal one are never read.
        assert_eq!(events.len(), 2);
        assert_eq!(*calls.lock().unwrap(), vec![false]);
    }

    #[tokio::test]
    async fn test_loop_transport_error_yields_error_and_no_finalize() {
        let fake = Fake::default();
        let calls = fake.finalize_calls.clone();
        let logged = fake.logged.clone();
        // A real reqwest::Error from an unparsable URL.
        let err = reqwest::Client::new()
            .get("http://[::")
            .build()
            .unwrap_err();
        let frames = frames_of(vec![Ok(frame(None, "a")), Err(err)]);
        let events: Vec<RawEvent> = process_frame_stream(frames, fake).collect().await;
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[1], RawEvent::Public(AiEvent::Error(_))));
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(*logged.lock().unwrap(), 1);
    }
}
