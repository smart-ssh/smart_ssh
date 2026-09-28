//! Wiremock-basierte Tests für `OpenAiCompatibleProvider` (Spec 0006,
//! Abschnitt 7, zweiter Block).

use std::sync::Arc;

use ai_providers::{OpenAiCompatibleProvider, ProviderBudgetGuard};
use futures::StreamExt;
use ssh_manager_core::ai::{default_action_schemas, AiError, AiEvent, AiProvider, SessionContext};
use ssh_manager_core::profiles::AiAction;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// Spec 0080, T3: prüft den tatsächlich gesendeten `max_tokens`-Wert im
/// Request-Body, statt (wie die übrigen Retry-Tests dieser Datei) nur die
/// Zahl der Requests über `.expect(n)` zu zählen — das allein würde eine
/// falsch verdoppelte Zahl nicht auffangen.
struct BodyContains(String);

impl wiremock::Match for BodyContains {
    fn matches(&self, request: &Request) -> bool {
        String::from_utf8_lossy(&request.body).contains(self.0.as_str())
    }
}

fn test_budget() -> Arc<ProviderBudgetGuard> {
    Arc::new(ProviderBudgetGuard::new())
}

fn empty_context() -> SessionContext {
    SessionContext {
        system_context: "Testkontext".to_string(),
        history: Vec::new(),
        available_actions: default_action_schemas(),
        max_tokens_hint: None,
    }
}

async fn mock_server_with_sse_body(sse_body: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body.to_string()),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn test_native_tool_calling_success_yields_action_proposed() {
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"Klar,\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"suggest_command\",\"arguments\":\"\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\": \\\"ls -la\\\"}\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n\
data: [DONE]\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events[0], AiEvent::TextDelta("Klar,".to_string()));
    assert_eq!(
        events[1],
        AiEvent::ActionProposed(AiAction::SuggestCommand {
            command: "ls -la".to_string()
        })
    );
    assert_eq!(events[2], AiEvent::Done);
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn test_fallback_mode_parses_action_block_after_stream_completes() {
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"Sicher. \"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"<!--ACTION-->{\\\"action\\\": \\\"suggest_command\\\", \\\"parameters\\\": {\\\"command\\\": \\\"df -h\\\"}}<!--/ACTION-->\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\" Fertig.\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "test-key",
        false,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    // Im Fallback-Modus wird nichts inkrementell gestreamt (s. Kommentar in
    // fallback.rs) — genau ein TextDelta mit dem bereinigten Text, dann die
    // Aktion, dann Done.
    assert_eq!(events.len(), 3);
    let AiEvent::TextDelta(text) = &events[0] else {
        panic!("erwartetes TextDelta, bekam {:?}", events[0]);
    };
    assert!(!text.contains("<!--ACTION-->"));
    assert!(text.contains("Sicher."));
    assert!(text.contains("Fertig."));
    assert_eq!(
        events[1],
        AiEvent::ActionProposed(AiAction::SuggestCommand {
            command: "df -h".to_string()
        })
    );
    assert_eq!(events[2], AiEvent::Done);
}

#[tokio::test]
async fn test_fallback_mode_treats_malformed_action_block_as_plain_text() {
    let full_text = "Text vor <!--ACTION-->{invalid<!--/ACTION--> Text danach";
    let sse_body = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\ndata: [DONE]\n\n",
        serde_json::to_string(full_text).unwrap()
    );
    let server = mock_server_with_sse_body(&sse_body).await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "test-key",
        false,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta(full_text.to_string()), AiEvent::Done]
    );
}

#[tokio::test]
async fn test_authentication_failure_maps_401_to_ai_error() {
    let server = MockServer::start().await;
    // `.expect(1)`: s. identischer Kommentar in
    // `tests/anthropic.rs::test_authentication_failure_maps_401_to_ai_error`.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "bad-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::AuthenticationFailed)]);
}

/// Spec 0051, Teil 1: s. identischer Kommentar in
/// `tests/anthropic.rs::test_429_with_retry_after_retries_and_then_succeeds`
/// — gilt "für alle 429-fähigen KI-Requests", also auch für den
/// OpenAI-kompatiblen Provider.
#[tokio::test]
async fn test_429_with_retry_after_retries_and_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("rate limited"),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse_body.to_string()),
        )
        .with_priority(2)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );
}

/// Spec 0051, Teil 1: s. identischer Kommentar in
/// `tests/anthropic.rs::test_persistent_429_gives_up_after_attempt_cap_with_rate_limited_error`.
#[tokio::test]
async fn test_persistent_429_gives_up_after_attempt_cap_with_rate_limited_error() {
    let server = MockServer::start().await;
    // `.expect(4)`: s. identischer Kommentar in
    // `tests/anthropic.rs::test_persistent_429_gives_up_after_attempt_cap_with_rate_limited_error`
    // zur Begründung, warum die reine Endzustands-Assertion allein
    // tautologisch wäre.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("rate limited"),
        )
        .expect(4)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);
}

/// Spec-Reviewer-Fund: s. identischer Kommentar in
/// `tests/anthropic.rs::test_retry_after_longer_than_total_budget_gives_up_without_extra_request`.
#[tokio::test]
async fn test_retry_after_longer_than_total_budget_gives_up_without_extra_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "3600")
                .set_body_string("rate limited"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::RateLimited)]);
}

/// Spec 0068, Teil 4: parallele `tool_calls` (zwei Indizes) liefern zwei
/// getrennte Aktionen in Reihenfolge.
#[tokio::test]
async fn test_parallel_tool_calls_yield_two_actions_in_order() {
    let sse_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"suggest_command\",\"arguments\":\"\"}},{\"index\":1,\"id\":\"c2\",\"type\":\"function\",\"function\":{\"name\":\"suggest_command\",\"arguments\":\"\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\": \\\"ls\\\"}\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"command\\\": \\\"uptime\\\"}\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n\
data: [DONE]\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "ls".to_string()
            }),
            AiEvent::ActionProposed(AiAction::SuggestCommand {
                command: "uptime".to_string()
            }),
            AiEvent::Done,
        ]
    );
}

/// Spec 0068, Teil 4 + 0065: zwei Tool-Calls, Antwort am Längenlimit
/// abgeschnitten (`finish_reason: length`) — KEINE der beiden Aktionen
/// wird freigegeben, auch nicht der vollständige erste Call; nach dem
/// einmaligen Retry mit demselben Ergebnis kommt ein sichtbarer Fehler.
#[tokio::test]
async fn test_parallel_tool_calls_cut_off_at_length_release_no_action() {
    let sse_body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"suggest_command\",\"arguments\":\"{\\\"command\\\": \\\"ls\\\"}\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"c2\",\"type\":\"function\",\"function\":{\"name\":\"suggest_command\",\"arguments\":\"{\\\"command\\\": \\\"rm -rf /var/lo\"}}]}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n\
data: [DONE]\n\n";
    let server = mock_server_with_sse_body(sse_body).await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "gpt-test",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AiEvent::ActionProposed(_))),
        "aus einer abgeschnittenen Antwort darf keine Aktion kommen: {events:?}"
    );
    assert_eq!(
        events.last(),
        Some(&AiEvent::Error(AiError::ResponseTruncated))
    );
}

/// T3 (Spec 0080, A1): zwei aufeinanderfolgende leere `length`-Antworten
/// (kein Text, kein Tool-Call) am OpenAI-kompatiblen Provider — ein
/// Nicht-OpenAI-Endpunkt mit unbekanntem Modell, Default 8192 (Spec 0080
/// §8, P1 Variante b). Zwei Requests: der erste mit dem Default (8192), der
/// zweite mit dem verdoppelten Budget (16384, gedeckelt am Modell-Maximum,
/// das hier zufällig ebenfalls 16384 ist). Am Ende `Error(ResponseTruncated)`,
/// nie `TextTruncated` — die leere Runde darf nicht kommentarlos als
/// "abgeschnittener Text" enden, s. Spec 0080 §1.
#[tokio::test]
async fn test_empty_length_round_retries_once_with_doubled_budget_then_errors() {
    let server = MockServer::start().await;
    let empty_length_body =
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":8192".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(empty_length_body),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":16384".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(empty_length_body),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    // `.expect(1)` auf beiden Mocks (Drop-Assertion von wiremock) prüft
    // bereits, dass GENAU der erwartete `max_tokens`-Wert in jedem der
    // beiden Requests stand — würde die Verdopplung ausbleiben oder
    // falsch rechnen, träfe keiner der beiden Mocks zweimal, wiremock
    // panickt dann beim Server-Drop.
    assert_eq!(events, vec![AiEvent::Error(AiError::ResponseTruncated)]);
    assert!(
        !events.iter().any(|e| matches!(e, AiEvent::TextTruncated)),
        "eine leere, abgeschnittene Runde darf nicht als TextTruncated enden: {events:?}"
    );
}

/// Spec-reviewer-Fund (ERHÖHT, Spec 0080, Review dieses Schritts): ein
/// `max_tokens_override` (Spec 0065, Teil 4), der bewusst über dem
/// Modell-Maximum liegt, darf der A1-Retry NICHT halbieren. Ohne den Fix
/// hätte `min(verdoppelt, kleineres Modell-Maximum)` den zweiten Request
/// mit WENIGER Budget geschickt als der Nutzer explizit eingestellt hat —
/// gerade in der Reasoning-Modell-Situation, für die Spec 0080 gedacht
/// ist. Modell-Maximum an diesem (Nicht-OpenAI-)Endpunkt ist 16384 (Spec
/// 0080 §8), der Override 32000 liegt bewusst darüber.
#[tokio::test]
async fn test_empty_length_retry_never_sends_less_than_an_explicit_max_tokens_override() {
    let server = MockServer::start().await;
    let empty_length_body =
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":32000".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(empty_length_body),
        )
        .expect(2)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        Some(32_000),
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    // `.expect(2)` oben ist der eigentliche Beweis: BEIDE Requests trugen
    // `"max_tokens":32000` — würde der zweite stattdessen 16384 (halbiert
    // statt beibehalten) schicken, träfe der Mock nur einmal, und
    // wiremock ließe den Server-Drop fehlschlagen.
    assert_eq!(events, vec![AiEvent::Error(AiError::ResponseTruncated)]);
}

/// Ein leerer `SessionContext` mit `max_tokens_hint` — für T7/T7b (Spec
/// 0087, A1.2b), die einen Nebenaufruf simulieren.
fn context_with_max_tokens_hint(hint: u32) -> SessionContext {
    let mut context = empty_context();
    context.max_tokens_hint = Some(hint);
    context
}

const SUCCESS_SSE_BODY: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

/// Spec 0087, T1 (A1.2): der vLLM-Fehlertext aus §1
/// (`max_tokens=8192 cannot be greater than max_model_len=4096. ...`) —
/// unbekanntes Modell an einem Nicht-OpenAI-Endpunkt, Default 8192 (Spec
/// 0080 §8). Der Nutzer erhält am Ende die Antwort statt eines Fehlers; die
/// zweite Anfrage trägt `max_tokens: 2048`, die erste 8192. *Gegenbeweis
/// (s. Bericht):* vor diesem Fix landete die erste 400-Antwort unverändert
/// als `AiEvent::Error(ProviderUnavailable(_))`, keine zweite Anfrage.
#[tokio::test]
async fn test_vllm_max_tokens_over_context_error_retries_with_smaller_budget_then_succeeds() {
    let server = MockServer::start().await;
    let error_body = r#"{"error":{"message":"max_tokens=8192 cannot be greater than max_model_len=4096. Please request fewer output tokens.","type":"BadRequestError"}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":8192".to_string()))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":2048".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SUCCESS_SSE_BODY),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );
}

/// Spec 0087, T2 (A1.1): der OpenAI-Prosa-Text „This model's maximum
/// context length is …“ — derselbe Retry wie T1, anderer Fehlertext.
#[tokio::test]
async fn test_openai_style_context_length_error_retries_with_smaller_budget_then_succeeds() {
    let server = MockServer::start().await;
    let error_body = r#"{"error":{"message":"This model's maximum context length is 4096 tokens. However, you requested 8192 tokens.","type":"invalid_request_error"}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":8192".to_string()))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":2048".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SUCCESS_SSE_BODY),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );
}

/// Spec 0087, T2b (A1.1): derselbe Fehlertyp wie T2, aber komplett
/// klein geschrieben (OpenRouter-Stil) — belegt, dass die Erkennung
/// case-insensitive arbeitet.
#[tokio::test]
async fn test_lowercase_openrouter_style_context_length_error_retries_then_succeeds() {
    let server = MockServer::start().await;
    let error_body = r#"{"error":{"message":"this endpoint's maximum context length is 4096 tokens. however, you requested about 8192 tokens (100 of text input, 8092 in the output)."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":8192".to_string()))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":2048".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SUCCESS_SSE_BODY),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );
}

/// Spec 0087, T3 (A1.1, Negativ): llama.cpps Fehlertext aus §1
/// (`exceed_context_size_error`) — llama.cpp prüft nur die Eingabe, ein
/// kleineres Budget hilft dort nicht (Nicht-Ziel dieser Spec). Genau EINE
/// Anfrage, `ProviderUnavailable`. *Scheitert, wenn die Erkennung auf
/// „context“ allein oder den Status allein reagiert.*
#[tokio::test]
async fn test_llama_cpp_exceed_context_size_error_does_not_retry() {
    let server = MockServer::start().await;
    let error_body = r#"{"error":{"code":400,"message":"request (931 tokens) exceeds the available context size (512 tokens), try increasing it","type":"exceed_context_size_error"}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert!(matches!(
        events.as_slice(),
        [AiEvent::Error(AiError::ProviderUnavailable(_))]
    ));
}

/// Spec 0087, T4 (A1.1, Negativ): ein 400, dessen Körper strukturell als
/// „Modell nicht gefunden“ erkennbar ist (`error.code == "model_not_found"`)
/// UND zusätzlich den Kontextgrenzen-Marker enthält — bleibt
/// `ModelNotFound`, eine Anfrage. Die Modell-nicht-gefunden-Prüfung geht
/// der Kontext-Erkennung vor (s. `crate::error::map_http_status`).
#[tokio::test]
async fn test_400_recognized_as_model_not_found_stays_model_not_found_even_with_context_marker() {
    let server = MockServer::start().await;
    let error_body = r#"{"error":{"code":"model_not_found","message":"maximum context length is 4096 tokens. However, you requested 8192."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert!(matches!(
        events.as_slice(),
        [AiEvent::Error(AiError::ModelNotFound(_))]
    ));
}

/// Spec 0087, T5 (A1.1, Negativ): derselbe vLLM-Kontexttext, aber mit
/// Status 413 statt 400 — die Erkennung wirkt nur für 400, andere
/// Statuscodes bleiben unverändert `ProviderUnavailable`, keine zweite
/// Anfrage.
#[tokio::test]
async fn test_413_with_context_length_wording_does_not_retry() {
    let server = MockServer::start().await;
    let error_body =
        r#"{"error":{"message":"max_tokens=8192 cannot be greater than max_model_len=4096."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(413).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert!(matches!(
        events.as_slice(),
        [AiEvent::Error(AiError::ProviderUnavailable(_))]
    ));
}

/// Spec 0087, T6 (A1.2a): ein explizit gesetzter `max_tokens_override`
/// (8192) darf der Kontext-Retry nie unterlaufen — genau EINE Anfrage,
/// `ContextTooLarge`. *Scheitert, wenn der Retry den Override unterläuft.*
#[tokio::test]
async fn test_context_error_with_explicit_override_does_not_retry() {
    let server = MockServer::start().await;
    let error_body =
        r#"{"error":{"message":"max_tokens=8192 cannot be greater than max_model_len=4096."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        Some(8192),
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::ContextTooLarge)]);
}

/// Spec 0087, T7 (A1.2b): ein Nebenaufruf mit `max_tokens_hint` ≤ 2048 —
/// das neue Budget (2048) wäre nicht kleiner als das gesendete, also kein
/// Retry: genau EINE Anfrage, `ContextTooLarge`.
#[tokio::test]
async fn test_context_error_with_side_call_hint_at_or_below_2048_does_not_retry() {
    let server = MockServer::start().await;
    let error_body =
        r#"{"error":{"message":"max_tokens=8192 cannot be greater than max_model_len=4096."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider
        .send(context_with_max_tokens_hint(1024))
        .collect()
        .await;

    assert_eq!(events, vec![AiEvent::Error(AiError::ContextTooLarge)]);
}

/// Spec 0087, T7b (A1.2a): `max_tokens_hint` (4096) UND `max_tokens_override`
/// (8192) sind beide gesetzt — der Hint hat Vorrang (s. `build_request_body`),
/// das Budget stammt damit NICHT aus dem Override: Retry mit 2048, die
/// Antwort kommt an. *Scheitert, wenn der Hint fälschlich als
/// Nutzereinstellung behandelt wird.*
#[tokio::test]
async fn test_context_error_with_hint_and_override_both_set_still_retries() {
    let server = MockServer::start().await;
    let error_body =
        r#"{"error":{"message":"max_tokens=8192 cannot be greater than max_model_len=4096."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":4096".to_string()))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":2048".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SUCCESS_SSE_BODY),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        Some(8192),
    );

    let events: Vec<AiEvent> = provider
        .send(context_with_max_tokens_hint(4096))
        .collect()
        .await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );
}

/// Spec 0087, T8 (A1.3): zwei aufeinanderfolgende Kontextgrenzen-Fehler —
/// genau ZWEI Anfragen (der Kontext-Retry mit 2048, dann kein dritter
/// Versuch mehr), `ContextTooLarge`, Stream endet.
#[tokio::test]
async fn test_two_consecutive_context_errors_end_with_context_too_large_after_exactly_two_requests()
{
    let server = MockServer::start().await;
    let error_body =
        r#"{"error":{"message":"max_tokens=8192 cannot be greater than max_model_len=4096."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(2)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::ContextTooLarge)]);
}

/// Spec 0087, T9 (A1.4, erster Punkt): 400 Kontextfehler (Retry mit 2048),
/// danach 200 mit `finish_reason: length` ohne Inhalt (die leere,
/// abgeschnittene Runde aus Spec 0080) — genau ZWEI Anfragen,
/// `ResponseTruncated`, kein dritter Request mit `max_tokens > 2048`.
#[tokio::test]
async fn test_context_error_then_empty_length_round_ends_response_truncated_no_third_request() {
    let server = MockServer::start().await;
    let error_body =
        r#"{"error":{"message":"max_tokens=8192 cannot be greater than max_model_len=4096."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":8192".to_string()))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    let empty_length_body =
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":2048".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(empty_length_body),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    // `.expect(1)` auf beiden Mocks beweist bereits die Request-Anzahl (2,
    // kein dritter Request mit `max_tokens: 4096` — der wäre auf keinem der
    // beiden Mocks gebunden und ließe wiremock beim Server-Drop mangels
    // Treffer fehlschlagen).
    assert_eq!(events, vec![AiEvent::Error(AiError::ResponseTruncated)]);
}

/// Spec 0087, T9b (A1.4, zweiter Punkt): umgekehrte Reihenfolge — zuerst
/// die leere `length`-Runde (Abschneide-Retry auf das verdoppelte Budget
/// 16384), danach ein Kontextfehler auf dieser zweiten Anfrage — genau ZWEI
/// Anfragen, `ResponseTruncated`, keine Anfrage mit `max_tokens: 2048` (der
/// `ContextTooLarge`-Hinweis wäre hier der falsche Rat).
#[tokio::test]
async fn test_empty_length_round_then_context_error_ends_response_truncated_no_2048_request() {
    let server = MockServer::start().await;
    let empty_length_body =
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":8192".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(empty_length_body),
        )
        .expect(1)
        .mount(&server)
        .await;
    let error_body =
        r#"{"error":{"message":"max_tokens=16384 cannot be greater than max_model_len=8192."}}"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContains("\"max_tokens\":16384".to_string()))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(events, vec![AiEvent::Error(AiError::ResponseTruncated)]);
}

/// Spec 0087, T10 (A1.2): ein Reasoning-Modell (`o1-mini`, offizielle
/// OpenAI-API) verlangt `max_completion_tokens` statt `max_tokens` — der
/// Kontext-Retry muss dasselbe Feld erneut setzen, nicht das klassische.
#[tokio::test]
async fn test_reasoning_model_context_retry_uses_max_completion_tokens_field() {
    let server = MockServer::start().await;
    let error_body = r#"{"error":{"message":"This model's maximum context length is 4096 tokens. However, you requested 32768 tokens.","type":"invalid_request_error"}}"#;
    // Die Feldnamen-Umschaltung auf `max_completion_tokens` (Spec 0065, Teil
    // 1) greift nur für `base_url.contains("api.openai.com")` — ein
    // literaler Pfadanteil mit diesem Substring macht das wahr, während der
    // Request trotzdem an den lokalen Mock-Server geht (kein echter
    // Netzwerkzugriff auf die offizielle API).
    let base_url = format!("{}/api.openai.com", server.uri());
    Mock::given(method("POST"))
        .and(path("/api.openai.com/chat/completions"))
        .and(BodyContains("\"max_completion_tokens\":32768".to_string()))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api.openai.com/chat/completions"))
        .and(BodyContains("\"max_completion_tokens\":2048".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SUCCESS_SSE_BODY),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        base_url,
        "o1-mini",
        "test-key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider.send(empty_context()).collect().await;

    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );
}
