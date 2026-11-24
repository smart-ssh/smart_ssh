//! The one retry stream behind every provider's `send()`: it connects, and
//! when the response ends in a truncated tool call (Spec 0065, part 3) or a
//! context-length rejection (Spec 0087, A1) it asks again once with another
//! `max_tokens`. Providers differ only in how they connect and parse
//! ([`ConnectFn`]), which body key carries the budget and whether a
//! context-limit retry exists at all.
//!
//! Security invariant 5: a truncated tool call is never forwarded. Whatever
//! the connection yields as [`RawEvent::RetryWithHigherMaxTokens`] is consumed
//! here; if the single retry is already used it ends in
//! `AiError::ResponseTruncated`.

use ssh_manager_core::sync::lock_tolerating_poison;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use uuid::Uuid;

use ssh_manager_core::ai::{AiError, AiEvent};

use crate::request_logging::log_context_limit_retry;

/// Internal stream element: a public event, or a signal for the retry layer
/// that never reaches the caller.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RawEvent {
    Public(AiEvent),
    /// The response ended in a truncated tool call (Spec 0065, part 3).
    RetryWithHigherMaxTokens,
    /// A 400 recognised as a context-length rejection (Spec 0087, A1.2); the
    /// retry layer decides whether and with which budget to retry. Only the
    /// OpenAI family emits it.
    ContextLimitError,
}

impl From<AiEvent> for RawEvent {
    fn from(event: AiEvent) -> Self {
        RawEvent::Public(event)
    }
}

pub(crate) type RawStream = Pin<Box<dyn Stream<Item = RawEvent> + Send>>;

pub(crate) fn to_raw_stream(inner: Pin<Box<dyn Stream<Item = AiEvent> + Send>>) -> RawStream {
    Box::pin(inner.map(RawEvent::Public))
}

/// A single internal event as a finished stream (counterpart of
/// `crate::error::error_stream` for the non-public `ContextLimitError`).
pub(crate) fn raw_event_stream(event: RawEvent) -> RawStream {
    Box::pin(futures::stream::once(async move { event }))
}

/// Spec 0087, A1.4: at most ONE of the two retries runs per `send()`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RetryKind {
    None,
    Truncation,
    Context,
}

/// Opens one connection with the given request body and returns its parsed
/// event stream.
pub(crate) type ConnectFn =
    Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = RawStream> + Send>> + Send + Sync>;

/// Everything the retry stream needs for one `send()` call. Cloneable: the
/// overload retry (issue #599) starts every attempt from a fresh copy.
#[derive(Clone)]
pub(crate) struct RetryParams {
    pub(crate) connect: ConnectFn,
    pub(crate) request_id: Uuid,
    pub(crate) body: Value,
    /// The budget the first request carries.
    pub(crate) max_tokens: u32,
    /// Model maximum (Spec 0065, part 1) — the retry cap.
    pub(crate) model_max_tokens: u32,
    /// JSON key of the budget in `body` (`max_tokens`, `max_completion_tokens`
    /// or `max_output_tokens`).
    pub(crate) max_tokens_field: &'static str,
    /// Spec 0087, A1.2a: the first budget comes from the user's
    /// `max_tokens_override` (a side-call hint never counts as one).
    pub(crate) max_tokens_is_user_override: bool,
    /// Spec 0087, A1.2: fixed budget of the context-limit retry; `None` for a
    /// provider without that retry.
    pub(crate) context_limit_retry_max_tokens: Option<u32>,
}

struct RetryState {
    params: RetryParams,
    max_tokens: u32,
    retry_used: RetryKind,
    inner: Option<RawStream>,
}

/// Issue #599: [`retry_stream`] plus the automatic retry on a pre-output
/// provider overload (`crate::overload`), and the request span that gives
/// every log line of this request its provider/model.
pub(crate) fn retry_stream_with_overload(
    provider: &'static str,
    model: &str,
    last_request: &std::sync::Mutex<Option<Uuid>>,
    params: RetryParams,
) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
    let request_id = params.request_id;
    *lock_tolerating_poison(last_request) = Some(request_id);
    let span = tracing::info_span!("ai_request", provider, model = %model);
    crate::overload::with_overload_retry(span, request_id, move || retry_stream(params.clone()))
}

/// One attempt of a `send()` call, including the single higher-`max_tokens`
/// (or context-limit) retry. Connects lazily on the first poll.
pub(crate) fn retry_stream(params: RetryParams) -> Pin<Box<dyn Stream<Item = AiEvent> + Send>> {
    let state = RetryState {
        max_tokens: params.max_tokens,
        params,
        retry_used: RetryKind::None,
        inner: None,
    };
    // The unfold state is `None` once a terminal error was emitted, so the
    // stream ends right after it without a separate "finished" flag.
    Box::pin(futures::stream::unfold(Some(state), |slot| async move {
        let mut state = slot?;
        loop {
            if state.inner.is_none() {
                let stream = (state.params.connect)(state.params.body.clone()).await;
                state.inner = Some(stream);
            }
            match state.inner.as_mut().expect("just set").next().await {
                Some(RawEvent::Public(event)) => return Some((event, Some(state))),
                Some(RawEvent::RetryWithHigherMaxTokens) => {
                    // Spec 0087, A1.4: if ANY retry already ran, no third
                    // request; the truncated call is never forwarded.
                    if state.retry_used != RetryKind::None {
                        return Some((AiEvent::Error(AiError::ResponseTruncated), None));
                    }
                    state.retry_used = RetryKind::Truncation;
                    // `max_tokens_override` is deliberately not capped at
                    // the model maximum for the first request; without the
                    // `.max(..)` an override above the maximum would be
                    // silently halved here (Spec 0080, 0087 A2.1). The
                    // retry then repeats the same value instead.
                    let doubled_and_capped = state
                        .max_tokens
                        .saturating_mul(2)
                        .min(state.params.model_max_tokens);
                    state.max_tokens = doubled_and_capped.max(state.max_tokens);
                    state.params.body[state.params.max_tokens_field] = json!(state.max_tokens);
                    state.inner = None;
                }
                Some(RawEvent::ContextLimitError) => match state.retry_used {
                    // Spec 0087, A1.4: after a used truncation retry the
                    // sent budget is already the doubled one, so "lower
                    // the budget" would be the wrong advice.
                    RetryKind::Truncation => {
                        return Some((AiEvent::Error(AiError::ResponseTruncated), None));
                    }
                    // Spec 0087, A1.3: the context retry failed too.
                    RetryKind::Context => {
                        return Some((AiEvent::Error(AiError::ContextTooLarge), None));
                    }
                    RetryKind::None => {
                        // Spec 0087, A1.2: the budget is not a user
                        // override and the retry budget is really smaller.
                        let retry_budget =
                            state
                                .params
                                .context_limit_retry_max_tokens
                                .filter(|budget| {
                                    !state.params.max_tokens_is_user_override
                                        && *budget < state.max_tokens
                                });
                        let Some(retry_budget) = retry_budget else {
                            return Some((AiEvent::Error(AiError::ContextTooLarge), None));
                        };
                        // Spec 0087, A1.6: debug line with old and new
                        // budget, before the retry request.
                        log_context_limit_retry(
                            state.params.request_id,
                            state.max_tokens,
                            retry_budget,
                        );
                        state.retry_used = RetryKind::Context;
                        state.max_tokens = retry_budget;
                        state.params.body[state.params.max_tokens_field] = json!(state.max_tokens);
                        state.inner = None;
                    }
                },
                None => return None,
            }
        }
    }))
}
