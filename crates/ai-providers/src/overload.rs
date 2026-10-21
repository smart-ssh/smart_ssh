//! Automatic retry on provider overload (Issue #599, Spec 0051 §6).
//!
//! Shared by all providers: wraps the stream of one `send()` call. When the
//! provider answers with [`AiError::Overloaded`] (HTTP 503/529 or an in-stream
//! overload error) **before anything of the response was emitted**, the same
//! request is made again after a wait, at most [`MAX_RETRIES`] times and at
//! most [`MAX_TOTAL_WAIT`] of waiting in total. After the first emitted event
//! (text, tool call, …) there is never a retry, so a command proposed in a
//! partial response cannot be proposed twice.
//!
//! The request body is rebuilt by the caller's factory from the same inputs,
//! i.e. resent unchanged (redaction already applied, as for the 429 retry).
//! Cancelling the turn drops the stream and with it a running wait.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use futures::StreamExt;
use ssh_manager_core::ai::{AiError, AiEvent};

pub(crate) const MAX_RETRIES: u32 = 3;
/// Base waits before retry 1, 2 and 3.
const BACKOFF_SECS: [u64; 3] = [2, 5, 10];
pub(crate) const MAX_TOTAL_WAIT: Duration = Duration::from_secs(30);

type AiStream = Pin<Box<dyn Stream<Item = AiEvent> + Send>>;

/// Wait before retry `attempt` (1-based): the provider's `Retry-After` if it
/// sent one, else the base wait plus up to 500 ms of jitter. `None` when the
/// wait would not fit into the remaining total budget — a longer
/// `Retry-After` is the provider asking not to be asked earlier.
pub(crate) fn overload_delay(
    attempt: u32,
    retry_after: Option<Duration>,
    already_waited: Duration,
) -> Option<Duration> {
    if attempt == 0 || attempt > MAX_RETRIES {
        return None;
    }
    let delay = retry_after.unwrap_or_else(|| {
        let jitter_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u64::from(d.subsec_millis()) % 500)
            .unwrap_or(0);
        Duration::from_secs(BACKOFF_SECS[(attempt - 1) as usize]) + Duration::from_millis(jitter_ms)
    });
    (already_waited.saturating_add(delay) <= MAX_TOTAL_WAIT).then_some(delay)
}

struct State<F> {
    make: F,
    inner: Option<AiStream>,
    retries: u32,
    waited: Duration,
    emitted: bool,
    wait: Option<Duration>,
    request_id: uuid::Uuid,
}

/// Runs `make()` and repeats it on pre-output overload (see module docs).
/// `span` carries provider/model/request id so that every log line of the
/// request inherits them; it is entered on each poll.
pub(crate) fn with_overload_retry<F>(
    span: tracing::Span,
    request_id: uuid::Uuid,
    make: F,
) -> AiStream
where
    F: Fn() -> AiStream + Send + 'static,
{
    let state = State {
        make,
        inner: None,
        retries: 0,
        waited: Duration::ZERO,
        emitted: false,
        wait: None,
        request_id,
    };
    let inner: AiStream = Box::pin(futures::stream::unfold(state, |mut state| async move {
        if let Some(delay) = state.wait.take() {
            tokio::time::sleep(delay).await;
            state.waited += delay;
        }
        if state.inner.is_none() {
            state.inner = Some((state.make)());
        }
        match state.inner.as_mut().expect("just set").next().await {
            None => None,
            Some(AiEvent::Error(AiError::Overloaded { retry_after })) if !state.emitted => {
                let attempt = state.retries + 1;
                let Some(delay) = overload_delay(attempt, retry_after, state.waited) else {
                    return Some((AiEvent::Error(AiError::Overloaded { retry_after }), state));
                };
                state.retries = attempt;
                state.inner = None;
                state.wait = Some(delay);
                tracing::warn!(
                    request_id = %state.request_id,
                    attempt,
                    max_attempts = MAX_RETRIES,
                    delay_ms = delay.as_millis() as u64,
                    "AI provider is overloaded — retrying with backoff",
                );
                let event = AiEvent::Retrying {
                    attempt,
                    max_attempts: MAX_RETRIES,
                    // Round up so the shown wait is never shorter than real.
                    delay_secs: delay.as_millis().div_ceil(1000) as u64,
                };
                Some((event, state))
            }
            Some(event) => {
                state.emitted = true;
                Some((event, state))
            }
        }
    }));
    Box::pin(InSpan { span, inner })
}

/// Enters `span` for every poll of `inner`.
struct InSpan {
    span: tracing::Span,
    inner: AiStream,
}

impl Stream for InSpan {
    type Item = AiEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<AiEvent>> {
        let this = self.get_mut();
        let _guard = this.span.enter();
        this.inner.as_mut().poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    fn overloaded() -> AiEvent {
        AiEvent::Error(AiError::Overloaded { retry_after: None })
    }

    fn run(
        script: Vec<Vec<AiEvent>>,
    ) -> (
        Arc<AtomicU32>,
        impl std::future::Future<Output = Vec<AiEvent>>,
    ) {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let script = Arc::new(script);
        let make = move || -> AiStream {
            let i = c.fetch_add(1, Ordering::SeqCst) as usize;
            let events = script.get(i).cloned().unwrap_or_default();
            Box::pin(futures::stream::iter(events))
        };
        let stream = with_overload_retry(tracing::Span::none(), uuid::Uuid::new_v4(), make);
        (calls, stream.collect::<Vec<_>>())
    }

    #[tokio::test(start_paused = true)]
    async fn test_retries_three_times_then_overloaded() {
        let (calls, fut) = run(vec![
            vec![overloaded()],
            vec![overloaded()],
            vec![overloaded()],
            vec![overloaded()],
        ]);
        let events = fut.await;
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        let retrying: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AiEvent::Retrying {
                    attempt,
                    max_attempts,
                    ..
                } => Some((*attempt, *max_attempts)),
                _ => None,
            })
            .collect();
        assert_eq!(retrying, vec![(1, 3), (2, 3), (3, 3)]);
        assert_eq!(events.last(), Some(&overloaded()));
    }

    #[tokio::test(start_paused = true)]
    async fn test_success_on_second_attempt_delivers_response() {
        let (calls, fut) = run(vec![
            vec![overloaded()],
            vec![AiEvent::TextDelta("hi".into()), AiEvent::Done],
        ]);
        let events = fut.await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(matches!(events[0], AiEvent::Retrying { attempt: 1, .. }));
        assert_eq!(events[1], AiEvent::TextDelta("hi".into()));
        assert_eq!(events[2], AiEvent::Done);
    }

    #[tokio::test(start_paused = true)]
    async fn test_no_retry_after_output_was_emitted() {
        let (calls, fut) = run(vec![
            vec![AiEvent::TextDelta("partial".into()), overloaded()],
            vec![AiEvent::TextDelta("again".into()), AiEvent::Done],
        ]);
        let events = fut.await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            events,
            vec![AiEvent::TextDelta("partial".into()), overloaded()]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_other_errors_are_not_retried() {
        let (calls, fut) = run(vec![vec![AiEvent::Error(AiError::RateLimited)]]);
        let events = fut.await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);
    }

    #[test]
    fn test_delay_respects_total_cap_and_retry_after() {
        // Backoff 2 s / 5 s / 10 s (+ < 0.5 s jitter) fits into 30 s.
        let d1 = overload_delay(1, None, Duration::ZERO).unwrap();
        assert!(d1 >= Duration::from_secs(2) && d1 < Duration::from_millis(2500));
        let d3 = overload_delay(3, None, Duration::from_secs(8)).unwrap();
        assert!(d3 >= Duration::from_secs(10));
        // Retry-After is honoured within the cap …
        assert_eq!(
            overload_delay(1, Some(Duration::from_secs(25)), Duration::ZERO),
            Some(Duration::from_secs(25))
        );
        // … and one that would exceed the 30 s total ends the retries.
        assert_eq!(
            overload_delay(2, Some(Duration::from_secs(25)), Duration::from_secs(10)),
            None
        );
        assert_eq!(overload_delay(4, None, Duration::ZERO), None);
    }

    #[tokio::test(start_paused = true)]
    async fn test_dropping_stream_during_wait_makes_no_further_request() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let make = move || -> AiStream {
            c.fetch_add(1, Ordering::SeqCst);
            Box::pin(futures::stream::iter(vec![overloaded()]))
        };
        let mut stream = with_overload_retry(tracing::Span::none(), uuid::Uuid::new_v4(), make);
        assert!(matches!(
            stream.next().await,
            Some(AiEvent::Retrying { .. })
        ));
        // Cancel while the wait would run: dropping the stream stops the turn.
        drop(stream);
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
