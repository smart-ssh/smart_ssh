//! The one HTTP attempt loop shared by all AI providers (issue #851).
//!
//! It owns: attempt counting, the 429 back-off with `Retry-After`
//! ([`crate::retry`], Spec 0051), the total-time cap, the send timeout,
//! recording of rate-limit headers, and the logging of transport errors,
//! retried 429s and error responses — always with the caller's full list of
//! secrets scrubbed. It contains no provider names: how a rejected response
//! is interpreted (web tools disabled, `stream_options` rejected, context
//! length exceeded, …) is up to the caller, which gets the already-read
//! [`RejectedResponse`] back.
//!
//! **Redaction invariant (Spec 0051):** every attempt is built by calling the
//! same `build_request` closure, which must produce the identical body each
//! time; a retry therefore never carries content the first attempt did not.

use reqwest::header::HeaderMap;
use uuid::Uuid;

use crate::error::ProviderErrorInfo;
use crate::error::{map_http_response, map_transport_error, provider_request_id, timeout_error};
use crate::request_logging::{
    log_provider_error_response_with_id, log_provider_rate_limited_retry,
    log_provider_transport_error,
};
use crate::sse::{
    read_error_body_with_timeout, read_error_body_with_timeout_capped, SSE_INACTIVITY_TIMEOUT,
};
use ssh_manager_core::ai::AiError;

/// What the caller hands to [`send_with_retry`] besides the request builder.
pub(crate) struct SendLoopParams<'a> {
    pub request_id: Uuid,
    /// Every secret the provider knows (API key, extra header values, …);
    /// scrubbed from all log lines written by the loop.
    pub secrets: &'a [&'a str],
    /// Called with the headers of every response (success and error) before
    /// the body is consumed — e.g. to feed the rate-limit budget.
    pub on_headers: Option<&'a (dyn Fn(&HeaderMap) + Sync)>,
}

/// A non-success response that is not being retried (any status other than
/// 429, a 429 that is out of attempts/time, or a 429 whose `Retry-After`
/// exceeds the remaining time). The body is already read.
pub(crate) struct RejectedResponse {
    pub status: reqwest::StatusCode,
    pub headers: HeaderMap,
    pub text: String,
    pub provider_request_id: Option<String>,
}

impl RejectedResponse {
    /// The generic mapping of this response to an [`AiError`].
    pub(crate) fn map(&self) -> AiError {
        map_http_response(self.status, &self.headers, &self.text)
    }

    /// Logs this response (status + scrubbed body) together with the error it
    /// ended up as (Spec 0049, finding 2: unit variants lose status/body).
    pub(crate) fn log(&self, request_id: Uuid, mapped: &AiError, secrets: &[&str]) {
        log_provider_error_response_with_id(
            request_id,
            self.provider_request_id.as_deref(),
            self.status.as_u16(),
            &self.text,
            mapped,
            secrets,
        );
    }
}

pub(crate) enum SendOutcome {
    Success(reqwest::Response),
    Rejected(RejectedResponse),
}

/// Sends the request built by `build_request` until it is answered with
/// something other than a retryable 429. Transport errors, timeouts and
/// quota-exhausted 429s are mapped and logged here and returned as `Err`.
pub(crate) async fn send_with_retry(
    build_request: impl Fn() -> reqwest::RequestBuilder,
    params: SendLoopParams<'_>,
) -> Result<SendOutcome, AiError> {
    let SendLoopParams {
        request_id,
        secrets,
        on_headers,
    } = params;
    let retry_start = tokio::time::Instant::now();
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        tracing::debug!(
            request_id = %request_id,
            attempt,
            "about to send HTTP request to AI provider",
        );
        let send = build_request().send();
        let response = match tokio::time::timeout(SSE_INACTIVITY_TIMEOUT, send).await {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                let mapped = map_transport_error(&err);
                log_provider_transport_error(request_id, &mapped, secrets);
                return Err(mapped);
            }
            // Without this limit a hanging connect would block the turn forever.
            Err(_elapsed) => {
                let mapped = timeout_error(SSE_INACTIVITY_TIMEOUT);
                log_provider_transport_error(request_id, &mapped, secrets);
                return Err(mapped);
            }
        };

        if let Some(on_headers) = on_headers {
            on_headers(response.headers());
        }

        let status = response.status();
        let headers = response.headers().clone();
        let provider_rid = provider_request_id(&headers);
        if status.as_u16() == 429 {
            let elapsed = retry_start.elapsed();
            let remaining = crate::retry::MAX_TOTAL_RETRY_TIME.saturating_sub(elapsed);
            let delay = crate::retry::retry_delay(&headers, attempt);
            // A `Retry-After` longer than the remaining budget is an explicit
            // hint not to retry earlier; an early request would be pointless.
            if crate::retry::retry_allowed(attempt + 1, elapsed) && delay <= remaining {
                // Bounded by `remaining`, so a hanging body cannot defeat the
                // total retry cap (Spec 0051).
                let text = read_error_body_with_timeout_capped(response.text(), remaining).await;
                // A 429 that is really an exhausted quota is a billing
                // problem — never retried (issue #599).
                if ProviderErrorInfo::from_body(&text).is_quota_exhausted() {
                    let rejected = RejectedResponse {
                        status,
                        headers,
                        text,
                        provider_request_id: provider_rid,
                    };
                    let mapped = rejected.map();
                    rejected.log(request_id, &mapped, secrets);
                    return Err(mapped);
                }
                log_provider_rate_limited_retry(request_id, attempt, &text, delay, secrets);
                tokio::time::sleep(delay).await;
                continue;
            }
        }

        if !status.is_success() {
            let text = read_error_body_with_timeout(response.text()).await;
            return Ok(SendOutcome::Rejected(RejectedResponse {
                status,
                headers,
                text,
                provider_request_id: provider_rid,
            }));
        }
        return Ok(SendOutcome::Success(response));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures::StreamExt;
    use ssh_manager_core::ai::{
        AiEvent, AiProvider, ChatMessage, MessageContent, RedactedContext, Role, SessionContext,
    };
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::anthropic::AnthropicProvider;
    use crate::openai_compatible::OpenAiCompatibleProvider;
    use crate::rate_limit_budget::ProviderBudgetGuard;
    use crate::retry::MAX_ATTEMPTS;

    const SECRET: &str = "sk-planted-secret-851";

    fn params<'a>(secrets: &'a [&'a str]) -> SendLoopParams<'a> {
        SendLoopParams {
            request_id: Uuid::new_v4(),
            secrets,
            on_headers: None,
        }
    }

    fn post(client: &reqwest::Client, uri: &str, body: &str) -> reqwest::RequestBuilder {
        client.post(uri).body(body.to_string())
    }

    fn context() -> RedactedContext {
        RedactedContext::for_tests(SessionContext {
            system_context: "System.".to_string(),
            history: vec![ChatMessage {
                role: Role::User,
                content: MessageContent::Text("hi".to_string()),
            }],
            available_actions: Vec::new(),
            max_tokens_hint: None,
        })
    }

    #[tokio::test]
    async fn test_429_with_retry_after_is_retried_up_to_max_attempts_with_identical_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let outcome = send_with_retry(
            || post(&client, &server.uri(), r#"{"same":"body"}"#),
            params(&[]),
        )
        .await;
        match outcome {
            Ok(SendOutcome::Rejected(r)) => assert_eq!(r.status.as_u16(), 429),
            _ => panic!("expected the exhausted 429 to be returned as rejected"),
        }
        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), MAX_ATTEMPTS as usize);
        assert!(received.iter().all(|r| r.body == received[0].body));
        assert_eq!(received[0].body, br#"{"same":"body"}"#);
    }

    #[tokio::test]
    async fn test_429_succeeds_after_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let outcome = send_with_retry(|| post(&client, &server.uri(), "x"), params(&[])).await;
        assert!(matches!(outcome, Ok(SendOutcome::Success(_))));
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }

    /// A `Retry-After` beyond the total-time cap must not be waited for.
    #[tokio::test]
    async fn test_total_time_cap_stops_retrying() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "3600"))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let outcome = send_with_retry(|| post(&client, &server.uri(), "x"), params(&[])).await;
        assert!(matches!(outcome, Ok(SendOutcome::Rejected(_))));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_4xx_other_than_429_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad"))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let outcome = send_with_retry(|| post(&client, &server.uri(), "x"), params(&[])).await;
        match outcome {
            Ok(SendOutcome::Rejected(r)) => {
                assert_eq!(r.status.as_u16(), 400);
                assert_eq!(r.text, "bad");
            }
            _ => panic!("expected rejected"),
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_transport_error_is_mapped_and_logged_once() {
        crate::test_support::install_test_subscriber_once();
        crate::test_support::clear_log_buffer();
        let client = reqwest::Client::new();
        let outcome =
            send_with_retry(|| post(&client, "http://127.0.0.1:1/", "x"), params(&[])).await;
        assert!(outcome.is_err());
        let warns = crate::test_support::log_lines_at_info_or_above();
        assert_eq!(
            warns
                .iter()
                .filter(|l| l.contains("transport/connection error"))
                .count(),
            1,
            "{warns:?}"
        );
    }

    fn assert_no_secret_in_log() {
        let log = crate::test_support::log_buffer_text();
        assert!(!log.contains(SECRET), "secret leaked into log: {log}");
        assert!(log.contains("REDACTED"), "no redaction marker: {log}");
    }

    async fn collect(provider: &dyn AiProvider) -> Vec<AiEvent> {
        provider.send(context()).collect().await
    }

    fn anthropic(base: &str) -> AnthropicProvider {
        AnthropicProvider::new(
            base,
            "claude-test",
            SECRET,
            true,
            Arc::new(ProviderBudgetGuard::new()),
            None,
        )
    }

    fn openai(base: &str) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(
            base,
            "model-x",
            SECRET,
            true,
            Vec::new(),
            Arc::new(ProviderBudgetGuard::new()),
            None,
        )
    }

    async fn error_body_server() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_string(format!(r#"{{"error":{{"message":"bad key {SECRET}"}}}}"#)),
            )
            .mount(&server)
            .await;
        server
    }

    fn userinfo_url() -> String {
        format!("http://user:{SECRET}@127.0.0.1:1")
    }

    #[tokio::test]
    async fn test_anthropic_never_logs_the_key_from_an_error_body() {
        crate::test_support::install_test_subscriber_once();
        crate::test_support::clear_log_buffer();
        let server = error_body_server().await;
        collect(&anthropic(&server.uri())).await;
        assert_no_secret_in_log();
    }

    #[tokio::test]
    async fn test_anthropic_never_logs_the_key_from_a_transport_error() {
        crate::test_support::install_test_subscriber_once();
        crate::test_support::clear_log_buffer();
        collect(&anthropic(&userinfo_url())).await;
        let log = crate::test_support::log_buffer_text();
        assert!(!log.contains(SECRET), "secret leaked into log: {log}");
        assert!(log.contains("transport/connection error"), "{log}");
    }

    #[tokio::test]
    async fn test_openai_compatible_never_logs_the_key_from_an_error_body() {
        crate::test_support::install_test_subscriber_once();
        crate::test_support::clear_log_buffer();
        let server = error_body_server().await;
        collect(&openai(&server.uri())).await;
        assert_no_secret_in_log();
    }

    #[tokio::test]
    async fn test_openai_compatible_never_logs_the_key_from_a_transport_error() {
        crate::test_support::install_test_subscriber_once();
        crate::test_support::clear_log_buffer();
        collect(&openai(&userinfo_url())).await;
        let log = crate::test_support::log_buffer_text();
        assert!(!log.contains(SECRET), "secret leaked into log: {log}");
        assert!(log.contains("transport/connection error"), "{log}");
    }
}
