//! Unit tests for `openai_compatible` (the provider module).

//! s. `crate::anthropic::tests` — identisches Timeout-Verhalten, hier
//! für den OpenAI-kompatiblen Provider gespiegelt.

use super::*;

/// Issue #545: a stored question is sent as assistant text with the
/// question and numbered option labels.
#[test]
fn test_question_history_entry_is_rendered_with_numbered_labels() {
    let content = MessageContent::Question {
        question_id: Uuid::new_v4(),
        question: "Which fix?".to_string(),
        options: vec![
            ssh_manager_core::profiles::AskUserOption {
                label: "Restart".to_string(),
                description: None,
            },
            ssh_manager_core::profiles::AskUserOption {
                label: "Reload".to_string(),
                description: Some("softer".to_string()),
            },
        ],
        multi_select: false,
    };
    assert_eq!(
        message_content_text(&content),
        "Which fix?\n1. Restart\n2. Reload"
    );
}

use ssh_manager_core::ai::default_action_schemas;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// Issue #504: an unsealed extension part is the fixed fenced placeholder
/// (Chat Completions).
#[test]
fn test_extension_part_renders_fixed_placeholder() {
    let p = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-4o",
        "k",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let ctx = crate::content::test_fixtures::context_with_extension_part("report.pdf");
    crate::content::test_fixtures::assert_extension_placeholder(&p.build_request_body(&ctx));
}

/// Spec 0087, T11 (A1.6): s. identischer Kommentar in
/// `tests/openai_compatible.rs::BodyContains` — hier lokal, weil dieser
/// Test (anders als die übrigen T1–T10, T15/T16) auf den Log-Puffer aus
/// `crate::test_support` zugreifen muss, der `pub(crate)` und damit für
/// die externe Integrationstest-Crate nicht erreichbar ist.
struct BodyContainsForTest(String);

impl wiremock::Match for BodyContainsForTest {
    fn matches(&self, request: &Request) -> bool {
        String::from_utf8_lossy(&request.body).contains(self.0.as_str())
    }
}

fn context_with_system(system_context: &str, actions: Vec<ActionSchema>) -> SessionContext {
    SessionContext {
        system_context: system_context.to_string(),
        history: vec![ssh_manager_core::ai::ChatMessage {
            role: Role::User,
            content: MessageContent::Text("hi".to_string()),
        }],
        available_actions: actions,
        max_tokens_hint: None,
    }
}

/// Issue #162 / Spec 0105: OpenAI-kompatible Provider bekommen nie ein
/// Web-Werkzeug und kein `web_search_options` — die Anfrage bleibt wie
/// vor der Web-Recherche.
#[test]
fn test_request_body_never_contains_a_web_tool() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-test",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let body =
        provider.build_request_body(&context_with_system("System.", default_action_schemas()));
    let names: Vec<String> = body["tools"]
        .as_array()
        .expect("tools muss gesetzt sein")
        .iter()
        .map(|t| {
            t["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(names.len(), default_action_schemas().len(), "{names:?}");
    assert!(!body.to_string().contains("web_search"), "{body}");
    assert!(!body.to_string().contains("web_fetch"), "{body}");
}

/// Spec 0087, T15 (A3.1, BL-0264): natives Tool-Calling, ein leerer bzw.
/// reiner Leerraum-System-Prompt — `messages[0]` muss die Nutzernachricht
/// sein, keine System-Nachricht vorangestellt. Analog zu Anthropics
/// `test_no_system_field_when_system_text_is_empty_with_native_tool_calling`
/// (Spec 0081). *Gegenbeweis (s. Bericht):* vor diesem Fix stand hier
/// immer `{"role":"system","content":""}` als erstes Element.
#[test]
fn test_no_system_message_when_system_text_is_empty_with_native_tool_calling() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-test",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    for system_context in ["", "  \n"] {
        let context = context_with_system(system_context, default_action_schemas());
        let body = provider.build_request_body(&context);
        let messages = body["messages"]
            .as_array()
            .expect("messages muss ein Array sein");
        assert_eq!(
            messages[0]["role"], "user",
            "bei system_context {system_context:?} darf keine System-Nachricht vor \
             der Nutzernachricht stehen: {body}"
        );
    }
}

/// Spec 0087, T16 (A3.1, BL-0264, Gegenprobe): Fallback-Modus (kein
/// natives Tool-Calling) — der Fallback-Zusatz ist nie leer, die
/// System-Nachricht mit dem Fallback-Zusatz bleibt also auch bei leerem
/// `system_context` erhalten.
#[test]
fn test_fallback_mode_keeps_system_message_even_with_empty_system_context() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-test",
        "key",
        false,
        Vec::new(),
        test_budget(),
        None,
    );
    let context = context_with_system("", default_action_schemas());

    let body = provider.build_request_body(&context);

    let messages = body["messages"]
        .as_array()
        .expect("messages muss ein Array sein");
    assert_eq!(messages[0]["role"], "system");
    assert!(
        !messages[0]["content"].as_str().unwrap().trim().is_empty(),
        "Fallback-Zusatz muss nicht-leeren Text liefern: {messages:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn test_inactivity_timeout_yields_network_error_instead_of_hanging_forever() {
    let never_yields: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> =
        Box::pin(futures::stream::pending());

    let mut events = process_frame_stream(
        never_yields,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    );
    let event = events.next().await;

    assert!(
        matches!(
            event,
            Some(RawEvent::Public(AiEvent::Error(AiError::Timeout { .. })))
        ),
        "expected Timeout after inactivity timeout, got {event:?}"
    );
}

/// Issue #694: keep-alive frames without content every 5 s must not keep
/// the stream alive: the content timeout fires after 120 s.
#[tokio::test(start_paused = true)]
async fn test_keep_alive_frames_do_not_restart_content_timeout() {
    let frames = crate::sse::periodic_frames(None, "", std::time::Duration::from_secs(5));
    let start = tokio::time::Instant::now();
    let mut events = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    );
    let event = events.next().await;
    assert!(
        matches!(
            event,
            Some(RawEvent::Public(AiEvent::Error(AiError::Timeout {
                secs: 120
            })))
        ),
        "{event:?}"
    );
    assert_eq!(start.elapsed(), std::time::Duration::from_secs(120));
}

/// Issue #694: a long answer with real deltas every 30 s (3 minutes)
/// is not cut off by the content timeout.
#[tokio::test(start_paused = true)]
async fn test_slow_deltas_over_three_minutes_complete_without_error() {
    let frames = crate::sse::periodic_frames(
        None,
        "{\"choices\":[{\"delta\":{\"content\":\"x\"}}]}",
        std::time::Duration::from_secs(30),
    );
    let events = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    );
    let collected: Vec<RawEvent> = events.take(6).collect().await;
    assert!(
        collected
            .iter()
            .all(|e| matches!(e, RawEvent::Public(AiEvent::TextDelta(_)))),
        "{collected:?}"
    );
}

/// Issue #694: deltas every 30 s never trip the content timeout, so
/// only the total deadline (enforced by the caller) can end the stream.
#[tokio::test(start_paused = true)]
async fn test_continuous_deltas_for_eleven_minutes_keep_streaming() {
    let frames = crate::sse::periodic_frames(
        None,
        "{\"choices\":[{\"delta\":{\"content\":\"x\"}}]}",
        std::time::Duration::from_secs(30),
    );
    let events = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    );
    let collected: Vec<RawEvent> = events.take(22).collect().await;
    assert_eq!(collected.len(), 22);
    assert!(collected
        .iter()
        .all(|e| matches!(e, RawEvent::Public(AiEvent::TextDelta(_)))));
}

fn frame(data: &str) -> Result<SseFrame, reqwest::Error> {
    Ok(SseFrame {
        event: None,
        data: data.to_string(),
    })
}

/// das OpenAI-kompatible Gegenstück zu
/// `anthropic::tests::test_message_delta_with_end_turn_stop_reason_is_logged`
/// — `choices[0].finish_reason` muss über den echten SSE-Parsing-Pfad
/// geloggt werden.
#[tokio::test]
async fn test_finish_reason_stop_is_logged() {
    crate::test_support::install_test_subscriber_once();
    crate::test_support::clear_log_buffer();

    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> =
        Box::pin(futures::stream::iter(vec![
            frame(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#),
            frame("[DONE]"),
        ]));
    let request_id = Uuid::new_v4();
    let events: Vec<RawEvent> =
        process_frame_stream(frames, true, request_id, String::new(), Vec::new(), None)
            .collect()
            .await;

    assert_eq!(events, vec![RawEvent::Public(AiEvent::Done)]);
    let log_text = crate::test_support::log_buffer_text();
    assert!(
        log_text.contains("\"stop\""),
        "finish_reason muss geloggt werden: {log_text}"
    );
    assert!(log_text.contains(&request_id.to_string()));
}

/// T1 (Spec 0080, A1): `finish_reason: "length"` OHNE jeden Inhalt (kein
/// Text, kein Tool-Call) — die leere, abgeschnittene Runde aus Spec
/// 0080 §1 (typischerweise ein Reasoning-Modell, dessen Denk-Tokens das
/// Budget vor jedem Text aufbrauchen). Löst jetzt den bestehenden Retry
/// aus statt (der alte, bis Spec 0080 gültige Befund) `TextTruncated` zu
/// werden, das ohne vorherigen Assistant-Eintrag im Frontend
/// stillschweigend verworfen wurde (Spec 0080 §1, letzter Punkt). Ersetzt
/// den alten `test_finish_reason_length_is_logged` (Spec 0080 §5: "Der
/// bestehende Test ... wird angepasst") — `finish_reason` bleibt
/// weiterhin geloggt, nur die Ereignis-Erwartung ändert sich.
#[tokio::test]
async fn test_finish_reason_length_with_no_content_triggers_retry_and_is_logged() {
    crate::test_support::install_test_subscriber_once();
    crate::test_support::clear_log_buffer();

    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> =
        Box::pin(futures::stream::iter(vec![
            frame(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#),
            frame("[DONE]"),
        ]));
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(events, vec![RawEvent::RetryWithHigherMaxTokens]);
    let log_text = crate::test_support::log_buffer_text();
    assert!(
        log_text.contains("length"),
        "finish_reason muss geloggt werden: {log_text}"
    );
}

/// T2 (Spec 0080, Wächter): `length` MIT tatsächlichem Textinhalt bleibt
/// `TextTruncated`, kein Retry — der neue Retry aus A1 gilt nur für die
/// LEERE Runde.
#[tokio::test]
async fn test_finish_reason_length_with_text_content_stays_text_truncated() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> =
        Box::pin(futures::stream::iter(vec![
            frame(r#"{"choices":[{"delta":{"content":"abc"}}]}"#),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#),
            frame("[DONE]"),
        ]));
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(
        events,
        vec![
            RawEvent::Public(AiEvent::TextDelta("abc".to_string())),
            RawEvent::Public(AiEvent::TextTruncated),
        ]
    );
}

/// T4 (Spec 0080, A1): wie T1, im Fallback-Modus (kein natives
/// Tool-Calling) — leere `length`-Runde löst denselben Retry aus.
#[tokio::test]
async fn test_fallback_mode_finish_reason_length_with_no_content_triggers_retry() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> =
        Box::pin(futures::stream::iter(vec![
            frame(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#),
            frame("[DONE]"),
        ]));
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        false,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(events, vec![RawEvent::RetryWithHigherMaxTokens]);
}

/// T5 (Spec 0080, Wächter): `content_filter` (kein `length`) löst auch
/// ohne jeden Inhalt keinen Retry aus (A1 greift nur für `length`).
/// Seit Issue #647 endet es als `Refused` statt `Done`.
#[tokio::test]
async fn test_content_filter_finish_reason_with_no_content_does_not_trigger_retry() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> =
        Box::pin(futures::stream::iter(vec![
            frame(r#"{"choices":[{"delta":{},"finish_reason":"content_filter"}]}"#),
            frame("[DONE]"),
        ]));
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(events, vec![RawEvent::Public(AiEvent::Refused)]);
}

/// T6 (Spec 0080, Wächter): Verbindungsende ohne jedes `finish_reason`,
/// ohne Inhalt — kein Retry aus A1 (der prüft explizit auf
/// `finish_reason: "length"`, ein fehlender Grund ist etwas anderes).
/// Grün auch mit dem alten Code.
#[tokio::test]
async fn test_abrupt_disconnect_without_finish_reason_and_no_content_does_not_trigger_empty_round_retry(
) {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![frame(r#"{"choices":[{"delta":{}}]}"#)]),
    );
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(events, vec![RawEvent::Public(AiEvent::Done)]);
}

/// Issue #647: `content_filter` ends as `Refused` — text kept, a pending
/// tool call dropped, no retry.
#[tokio::test]
async fn test_content_filter_is_refused_without_retry_or_tool_call() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![
            frame(r#"{"choices":[{"delta":{"content":"No."}}]}"#),
            frame(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"suggest_command","arguments":"{\"command\":\"ls\"}"}}]}}]}"#,
            ),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"content_filter"}]}"#),
            frame("[DONE]"),
        ]),
    );
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;
    assert_eq!(
        events,
        vec![
            RawEvent::Public(AiEvent::TextDelta("No.".to_string())),
            RawEvent::Public(AiEvent::Refused),
        ]
    );
}

/// T7 (Spec 0080, Wächter): `length` mit Inhalt aus reinem Leerraum —
/// es GAB Text (`text_delta_total_len > 0`), also kein Retry aus A1,
/// bleibt `TextTruncated`. Grün auch mit dem alten Code.
#[tokio::test]
async fn test_length_with_whitespace_only_content_does_not_trigger_empty_round_retry() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> =
        Box::pin(futures::stream::iter(vec![
            frame(r#"{"choices":[{"delta":{"content":"   "}}]}"#),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#),
            frame("[DONE]"),
        ]));
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(
        events,
        vec![
            RawEvent::Public(AiEvent::TextDelta("   ".to_string())),
            RawEvent::Public(AiEvent::TextTruncated),
        ]
    );
}

/// T8 (Spec 0080, Wächter): `length`, kein Text, aber ein SYNTAKTISCH
/// UNVOLLSTÄNDIGES Tool-Call-Argument (kein valides JSON, anders als
/// `test_truncated_but_parseable_tool_call_is_never_forwarded_and_
/// triggers_retry` unten, deren Argument-JSON zufällig bereits
/// vollständig ist) — bestehender Tool-Call-Pfad greift trotzdem
/// zuerst, A1s neue Prüfung (die explizit `self.tool_calls.is_empty()`
/// verlangt) wird gar nicht erreicht. Spec-reviewer-Fund (ERHÖHT,
/// Review dieses Schritts): ohne diesen Test hatte T8 keinen echten
/// Wächter für den "halben" (nicht bloß den vollständigen) Fall.
#[tokio::test]
async fn test_length_with_no_text_and_a_syntactically_incomplete_tool_call_triggers_retry_not_forward(
) {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![
            frame(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"suggest_command","arguments":"{\"command\": \"rm -rf /var/log/ap"}}]}}]}"#,
            ),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#),
            frame("[DONE]"),
        ]),
    );

    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(
        events,
        vec![RawEvent::RetryWithHigherMaxTokens],
        "ein syntaktisch unvollständiger Tool-Call darf bei length nie geparst/freigegeben werden"
    );
}

/// T9 (Spec 0080, Wächter): Fallback-Modus, ein gültig aussehender
/// Aktionsblock steckt in einem Denk-Delta (`reasoning_content`), der
/// eigentliche Inhalt (`content`) bleibt leer — Denk-Deltas werden nie
/// an `fallback_text` angehängt (A4-Invariante), der Aktions-Parser
/// sieht davon also nichts: keine Aktion, kein Text. Grün auch mit dem
/// alten Code (der `reasoning_content` schon vor A4 ignorierte).
#[tokio::test]
async fn test_fallback_mode_action_like_content_in_reasoning_delta_is_never_parsed_as_action() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![
            frame(
                r#"{"choices":[{"delta":{"reasoning_content":"<!--ACTION-->{\"action\": \"suggest_command\", \"parameters\": {\"command\": \"ls\"}}<!--/ACTION-->"}}]}"#,
            ),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#),
            frame("[DONE]"),
        ]),
    );
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        false,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(events, vec![RawEvent::Public(AiEvent::Done)]);
}

/// T10 (Spec 0080, A4): nur Denk-Deltas, dann `stop` — kein Text,
/// `reasoning_len > 0` und `text_len = 0` im Rundenabschluss-Log.
#[tokio::test]
async fn test_only_reasoning_deltas_logs_reasoning_len_with_zero_text_len() {
    crate::test_support::install_test_subscriber_once();
    crate::test_support::clear_log_buffer();

    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> =
        Box::pin(futures::stream::iter(vec![
            frame(r#"{"choices":[{"delta":{"reasoning_content":"denk denk denk"}}]}"#),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#),
            frame("[DONE]"),
        ]));
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(events, vec![RawEvent::Public(AiEvent::Done)]);
    let log_text = crate::test_support::log_buffer_text();
    assert!(
        log_text.contains("\"text_len\":0"),
        "text_len muss auch bei 0 geloggt werden: {log_text}"
    );
    assert!(
        log_text.contains("\"reasoning_len\":14"),
        "reasoning_len muss die Länge der Denk-Deltas tragen: {log_text}"
    );
}

/// Pflicht-Regressionstest, Spec 0065 §3 (sicherheitskritisch) — das
/// OpenAI-kompatible Gegenstück zu
/// `anthropic::tests::test_truncated_but_parseable_tool_call_is_never_forwarded_and_triggers_retry`.
/// Ist-Befund vor diesem Fix: `finalize()` versuchte akkumulierte
/// `tool_calls` IMMER zu parsen, unabhängig von `finish_reason` — ein
/// zufällig vollständiges Argument-JSON wurde bedingungslos als
/// `ActionProposed` freigegeben.
#[tokio::test]
async fn test_truncated_but_parseable_tool_call_is_never_forwarded_and_triggers_retry() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![
            frame(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"suggest_command","arguments":"{\"command\":\"rm -rf /var/log/app\"}"}}]}}]}"#,
            ),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#),
            frame("[DONE]"),
        ]),
    );

    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(
        events,
        vec![RawEvent::RetryWithHigherMaxTokens],
        "ein durch finish_reason=length abgeschnittener Tool-Call darf \
         nie als ActionProposed/Error freigegeben werden"
    );
}

/// Gegenprobe: identischer Tool-Call, aber `finish_reason: "tool_calls"`
/// (normaler, erfolgreicher Abschluss) — muss ganz normal durchgehen.
#[tokio::test]
async fn test_complete_tool_call_with_normal_finish_reason_is_forwarded() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![
            frame(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"suggest_command","arguments":"{\"command\":\"ls -la\"}"}}]}}]}"#,
            ),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#),
            frame("[DONE]"),
        ]),
    );

    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(
        events.len(),
        2,
        "erwartet: ActionProposed + Done, bekam {events:?}"
    );
    assert!(matches!(
        events[0],
        RawEvent::Public(AiEvent::ActionProposed(_))
    ));
    assert_eq!(events[1], RawEvent::Public(AiEvent::Done));
}

/// Issue #604: unknown tool and a known tool with a missing required
/// field are non-fatal; the valid call of the same response is kept.
#[tokio::test]
async fn test_invalid_tool_calls_are_non_fatal_and_valid_call_still_forwarded() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![
            frame(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"date","arguments":""}},{"index":1,"function":{"name":"suggest_command","arguments":"{\"nothing\":\"rm -rf /\"}"}},{"index":2,"function":{"name":"suggest_command","arguments":"{\"command\":\"ls\"}"}}]}}]}"#,
            ),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#),
            frame("[DONE]"),
        ]),
    );
    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;
    assert!(
        matches!(
            &events[0],
            RawEvent::Public(AiEvent::InvalidToolCall { tool_name, .. }) if tool_name == "date"
        ),
        "{events:?}"
    );
    assert!(
        matches!(
            &events[1],
            RawEvent::Public(AiEvent::InvalidToolCall { reason, .. })
                if *reason == ssh_manager_core::ai::InvalidToolCallReason::MissingField("command".into())
        ),
        "{events:?}"
    );
    assert!(matches!(
        events[2],
        RawEvent::Public(AiEvent::ActionProposed(_))
    ));
    assert_eq!(events[3], RawEvent::Public(AiEvent::Done));
}

/// Spec 0065 §3: die Verbindung bricht mitten in einem Tool-Call ab,
/// OHNE je `[DONE]` oder ein `finish_reason` zu liefern — muss ebenso
/// den Retry auslösen statt (wie vor diesem Fix) das unvollständige
/// Argument-Fragment einfach zu parsen.
#[tokio::test]
async fn test_abrupt_disconnect_mid_tool_call_triggers_retry_not_silent_parse() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![frame(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"suggest_command","arguments":"{\"command\":\"rm -rf /var/log/ap"}}]}}]}"#,
        )]),
    );

    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(events, vec![RawEvent::RetryWithHigherMaxTokens]);
}

/// Spec-reviewer-Fund (ERHÖHT, Review dieses Schritts): ein Gateway, das
/// `finish_reason` mit abweichender Schreibweise liefert (hier
/// `"MAX_TOKENS"` statt `"length"`), darf einen vollständig
/// akkumulierten Tool-Call nicht freigeben — nur `"stop"`/`"tool_calls"`
/// gelten als vollständig (Allowlist statt Denylist).
#[tokio::test]
async fn test_unrecognized_finish_reason_is_treated_as_truncated_not_complete() {
    let frames: Pin<Box<dyn Stream<Item = Result<SseFrame, reqwest::Error>> + Send>> = Box::pin(
        futures::stream::iter(vec![
            frame(
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"suggest_command","arguments":"{\"command\":\"rm -rf /var/log/app\"}"}}]}}]}"#,
            ),
            frame(r#"{"choices":[{"delta":{},"finish_reason":"MAX_TOKENS"}]}"#),
            frame("[DONE]"),
        ]),
    );

    let events: Vec<RawEvent> = process_frame_stream(
        frames,
        true,
        Uuid::new_v4(),
        String::new(),
        Vec::new(),
        None,
    )
    .collect()
    .await;

    assert_eq!(events, vec![RawEvent::RetryWithHigherMaxTokens]);
}

fn test_budget() -> std::sync::Arc<crate::rate_limit_budget::ProviderBudgetGuard> {
    std::sync::Arc::new(crate::rate_limit_budget::ProviderBudgetGuard::new())
}

fn context_with_actions(actions: Vec<ActionSchema>) -> SessionContext {
    SessionContext {
        system_context: "Hi.".to_string(),
        history: vec![ssh_manager_core::ai::ChatMessage {
            role: Role::User,
            content: MessageContent::Text("hi".to_string()),
        }],
        available_actions: actions,
        max_tokens_hint: None,
    }
}

/// Spec 0065, Teil 1: für die offizielle OpenAI-API modellabhängig,
/// verifiziert gegen developers.openai.com/api/docs/models.
#[test]
fn test_build_request_body_uses_model_aware_max_tokens_for_official_openai() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-6-astra",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let context = context_with_actions(Vec::new());

    let body = provider.build_request_body(&context);

    // Spec-reviewer-Fund (ERHÖHT): der DEFAULT ist bewusst kleiner als
    // das Modell-Maximum (128K), sonst hätte der Retry aus Teil 3
    // keinen Spielraum, s. `openai_compatible_default_max_tokens`.
    assert_eq!(body["max_tokens"], 32_000);
}

/// Gegenprobe: ein generisches/selbstgehostetes Gateway bekommt den
/// konservativen Fallback, unabhängig vom `model`-Namen — Spec 0065 §1:
/// "vorsichtiger" als bei Anthropic, das tatsächliche Output-Maximum
/// ist von hier aus nicht bekannt.
#[test]
fn test_build_request_body_stays_conservative_for_non_openai_endpoint() {
    let provider = OpenAiCompatibleProvider::new(
        "http://localhost:11434/v1",
        "gpt-6-astra",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let context = context_with_actions(Vec::new());

    let body = provider.build_request_body(&context);

    assert_eq!(
        body["max_tokens"],
        OPENAI_COMPATIBLE_NON_OPENAI_ENDPOINT_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS / 2
    );
}

/// T17 (Spec 0080 §8, Klarstellung P1 Variante b): ein unbekanntes
/// Modell an einem Nicht-OpenAI-Endpunkt ohne Override schickt jetzt
/// **8192** (vorher 2048) — bewusst der Literalwert, nicht über die
/// Konstante hergeleitet (anders als der Test oben), damit ein
/// versehentlich mitgeänderter Konstantenwert dieselbe Regression nicht
/// tautologisch grün durchließe.
#[test]
fn test_unknown_model_on_non_openai_endpoint_sends_8192_by_default() {
    let provider = OpenAiCompatibleProvider::new(
        "http://localhost:11434/v1",
        "some-unknown-model",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let context = context_with_actions(Vec::new());

    let body = provider.build_request_body(&context);

    assert_eq!(body["max_tokens"], 8192);
}

/// Spec 0065, Teil 1: ein Nebenaufruf (`max_tokens_hint`) überschreibt
/// den modellabhängigen Default.
#[test]
fn test_build_request_body_honors_max_tokens_hint_over_model_default() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-6-astra",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let mut context = context_with_actions(Vec::new());
    context.max_tokens_hint = Some(4096);

    let body = provider.build_request_body(&context);

    assert_eq!(body["max_tokens"], 4096);
}

/// Spec 0065, Teil 4: der Nutzer-Override überschreibt den
/// modellabhängigen Default für den Haupt-Chat.
#[test]
fn test_build_request_body_honors_provider_level_max_tokens_override() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-6-astra",
        "key",
        true,
        Vec::new(),
        test_budget(),
        Some(20_000),
    );
    let context = context_with_actions(Vec::new());

    let body = provider.build_request_body(&context);

    assert_eq!(body["max_tokens"], 20_000);
}

/// Gegenprobe: ein Nebenaufruf-`max_tokens_hint` hat weiterhin Vorrang
/// vor dem Provider-Override.
#[test]
fn test_side_call_hint_still_wins_over_provider_level_override() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-6-astra",
        "key",
        true,
        Vec::new(),
        test_budget(),
        Some(20_000),
    );
    let mut context = context_with_actions(Vec::new());
    context.max_tokens_hint = Some(4096);

    let body = provider.build_request_body(&context);

    assert_eq!(body["max_tokens"], 4096);
}

/// Spec-reviewer-Fund (ERHÖHT, Review dieses Schritts): ein UNBEKANNTER
/// Modellname auf der offiziellen OpenAI-API muss auf den konservativen
/// Fallback fallen, NICHT auf den größten bekannten Wert (128K) — vorher
/// war die Fallback-Richtung invertiert.
///
/// Spec-reviewer-Fund (Spec 0080, Review dieses Schritts): dieser Fall
/// bleibt bei 4096 — die Anhebung aus Spec 0080 §8/P1(b) gilt wörtlich
/// nur für Endpunkte AUSSER der offiziellen OpenAI-API, s.
/// `OPENAI_COMPATIBLE_OFFICIAL_API_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS`.
#[test]
fn test_unknown_model_on_official_openai_falls_back_conservatively_not_to_the_largest_value() {
    let max = openai_compatible_model_max_output_tokens(
        "https://api.openai.com/v1",
        "some-future-unlisted-model",
    );
    assert_eq!(
        max,
        OPENAI_COMPATIBLE_OFFICIAL_API_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS
    );
    assert_eq!(max, 4_096);
}

/// Spec-reviewer-Fund (ERHÖHT, Review dieses Schritts): der Default
/// muss für jeden Bucket strikt kleiner als das Modell-Maximum sein,
/// sonst hat der Retry aus Teil 3 keinen Spielraum.
#[test]
fn test_default_max_tokens_always_leaves_headroom_below_the_model_maximum() {
    let base_url = "https://api.openai.com/v1";
    for model in ["gpt-3.5-turbo", "gpt-4o", "gpt-4-0613", "o1", "gpt-6-astra"] {
        let default = openai_compatible_default_max_tokens(base_url, model);
        let max = openai_compatible_model_max_output_tokens(base_url, model);
        assert!(
            default < max,
            "Default ({default}) muss für {model} kleiner als das Modell-Maximum ({max}) sein"
        );
    }
}

/// Spec-reviewer-Fund (ERHÖHT, Review dieses Schritts): Reasoning-
/// Modelle (o1/o3/gpt-5.x) verlangen `max_completion_tokens` statt
/// `max_tokens` — vorher sendete der Provider unconditional `max_tokens`
/// (Regression gegenüber dem alten Verhalten, das gar kein `max_tokens`
/// setzte).
#[test]
fn test_reasoning_models_use_max_completion_tokens_field_on_official_openai() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "o1-mini",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let context = context_with_actions(Vec::new());

    let body = provider.build_request_body(&context);

    assert!(body.get("max_tokens").is_none());
    assert!(body.get("max_completion_tokens").is_some());
}

/// Gegenprobe: ein normales (Nicht-Reasoning-)Modell behält das
/// klassische `max_tokens`-Feld.
#[test]
fn test_non_reasoning_models_use_classic_max_tokens_field() {
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com/v1",
        "gpt-4o",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let context = context_with_actions(Vec::new());

    let body = provider.build_request_body(&context);

    assert!(body.get("max_completion_tokens").is_none());
    assert!(body.get("max_tokens").is_some());
}

/// Gegenprobe: ein generisches Gateway behält `max_tokens`, selbst wenn
/// der Modellname zufällig wie ein Reasoning-Modell aussieht — die
/// Feldnamen-Umschaltung gilt nur für die offizielle OpenAI-API (s.
/// `openai_max_tokens_field_name`-Doc-Kommentar).
#[test]
fn test_generic_gateway_keeps_classic_field_even_for_an_o1_like_model_name() {
    let provider = OpenAiCompatibleProvider::new(
        "http://localhost:11434/v1",
        "o1-mini",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    );
    let context = context_with_actions(Vec::new());

    let body = provider.build_request_body(&context);

    assert!(body.get("max_completion_tokens").is_none());
    assert!(body.get("max_tokens").is_some());
}

/// Spec 0087, T11 (A1.6): vor dem Kontext-Retry existiert eine
/// Fehler-Logzeile mit Status 400, der eingebettete API-Key erscheint
/// darin nur redigiert; nach dem erfolgreichen Retry existiert eine
/// Debug-Zeile, die das alte (8192) und das neue (2048) Budget enthält.
/// *Scheitert, wenn der Retry-Pfad das Loggen überspringt* — als
/// Wiremock-Test statt Direktaufruf, weil beide Log-Aufrufe im echten
/// `send()`-Retry-Pfad liegen (`connect_and_stream`/das `unfold` in
/// `send()`), nicht in einer isoliert testbaren Hilfsfunktion.
#[tokio::test]
async fn test_context_limit_retry_logs_error_then_debug_line_with_old_and_new_budget() {
    crate::test_support::install_test_subscriber_once();
    crate::test_support::clear_log_buffer();

    let server = MockServer::start().await;
    let api_key = "sk-test-secret-key-123";
    let error_body = format!(
        "{{\"error\":{{\"message\":\"max_tokens=8192 cannot be greater than max_model_len=4096 (key {api_key})\"}}}}"
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContainsForTest("\"max_tokens\":8192".to_string()))
        .respond_with(ResponseTemplate::new(400).set_body_string(error_body))
        .expect(1)
        .mount(&server)
        .await;
    let success_body =
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(BodyContainsForTest("\"max_tokens\":2048".to_string()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(success_body),
        )
        .expect(1)
        .mount(&server)
        .await;
    let provider = OpenAiCompatibleProvider::new(
        server.uri(),
        "some-unknown-model",
        api_key,
        true,
        Vec::new(),
        test_budget(),
        None,
    );

    let events: Vec<AiEvent> = provider
        .send(context_with_system("Testkontext", Vec::new()))
        .collect()
        .await;
    assert_eq!(
        events,
        vec![AiEvent::TextDelta("ok".to_string()), AiEvent::Done]
    );

    let log_text = crate::test_support::log_buffer_text();
    assert!(
        log_text.contains("400"),
        "Fehler-Logzeile mit Status 400 fehlt: {log_text}"
    );
    assert!(
        !log_text.contains(api_key),
        "der API-Key darf nicht im Log stehen: {log_text}"
    );
    assert!(
        log_text.contains("REDACTED"),
        "Redaction-Platzhalter fehlt: {log_text}"
    );
    assert!(
        log_text.contains("8192"),
        "altes Budget fehlt in der Debug-Zeile: {log_text}"
    );
    assert!(
        log_text.contains("2048"),
        "neues Budget fehlt in der Debug-Zeile: {log_text}"
    );
}

/// Issue #245 (Spec 0066): history `assistant (tool call)`, action result,
/// queued user text, queued user text.
fn history_with_queued(queued: &[&str]) -> Vec<ssh_manager_core::ai::ChatMessage> {
    use ssh_manager_core::ai::ChatMessage;
    let mut history = vec![
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text("list files".to_string()),
        },
        ChatMessage {
            role: Role::Assistant,
            content: MessageContent::Text("running ls".to_string()),
        },
        ChatMessage {
            role: Role::ActionResult,
            content: MessageContent::CommandResult {
                command: "ls".to_string(),
                output: ssh_manager_core::ssh::CommandOutput {
                    stdout: b"file-a".to_vec(),
                    stderr: Vec::new(),
                    exit_code: Some(0),
                    truncated: false,
                },
                cancelled: false,
                waiting_for_input: false,
            },
        },
    ];
    for text in queued {
        history.push(ChatMessage {
            role: Role::User,
            content: MessageContent::Text((*text).to_string()),
        });
    }
    history
}

fn history_context(history: Vec<ssh_manager_core::ai::ChatMessage>) -> SessionContext {
    SessionContext {
        system_context: "Hi.".to_string(),
        history,
        available_actions: Vec::new(),
        max_tokens_hint: None,
    }
}

/// Deterministic pseudo-random role sequences (no extra dev-dependency).
fn random_histories() -> Vec<Vec<ssh_manager_core::ai::ChatMessage>> {
    use ssh_manager_core::ai::ChatMessage;
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut all = Vec::new();
    for _ in 0..200 {
        let mut history = Vec::new();
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let len = 1 + (state >> 60) as usize;
        for i in 0..len {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let role = match (state >> 33) % 3 {
                0 => Role::User,
                1 => Role::Assistant,
                _ => Role::ActionResult,
            };
            let content = if role == Role::ActionResult {
                MessageContent::ActionRejected {
                    command: format!("cmd{i}"),
                    reason: ssh_manager_core::ai::RejectionReason::User,
                }
            } else {
                MessageContent::Text(format!("m{i}"))
            };
            history.push(ChatMessage { role, content });
        }
        all.push(history);
    }
    all
}

fn combine_provider() -> OpenAiCompatibleProvider {
    OpenAiCompatibleProvider::new(
        "https://example.test/v1",
        "model",
        "key",
        true,
        Vec::new(),
        test_budget(),
        None,
    )
}

#[test]
fn test_action_result_and_queued_text_become_one_user_message() {
    let body = combine_provider()
        .build_request_body(&history_context(history_with_queued(&["also check /tmp"])));
    let messages = body["messages"].as_array().unwrap();
    // system, user, assistant, one combined user
    assert_eq!(messages.len(), 4, "{body}");
    let last = &messages[3];
    assert_eq!(last["role"], "user");
    let text = last["content"].as_str().unwrap();
    let result_at = text.find("<command_execution_result>").unwrap();
    let queued_at = text.find("also check /tmp").unwrap();
    assert!(result_at < queued_at);
    assert!(text.ends_with("</command_execution_result>\n\nalso check /tmp"));
    // The queued text stays outside the fence / result element.
    assert!(!text[result_at..queued_at].contains("also check"));
}

#[test]
fn test_two_queued_messages_in_a_row_produce_one_user_message() {
    let body = combine_provider()
        .build_request_body(&history_context(history_with_queued(&["one", "two"])));
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4, "{body}");
    assert!(messages[3]["content"]
        .as_str()
        .unwrap()
        .ends_with("\n\none\n\ntwo"));
}

#[test]
fn test_no_request_body_has_two_adjacent_messages_with_the_same_role() {
    let provider = combine_provider();
    for history in random_histories() {
        let body = provider.build_request_body(&history_context(history));
        let roles: Vec<&str> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        for pair in roles.windows(2) {
            assert_ne!(pair[0], pair[1], "{roles:?}");
        }
    }
}

/// Issue #327: a stop in the waiting state asks for a non-interactive
/// variant; a plain stop or a normal result carries no such note.
#[test]
fn test_waiting_for_input_note_only_on_stop_while_waiting() {
    let output = CommandOutput {
        stdout: b"Continue? [Y/n] ".to_vec(),
        stderr: Vec::new(),
        exit_code: None,
        truncated: false,
    };
    let waiting = format_command_result("apt install x", &output, true, true);
    assert!(waiting.contains("<waiting_for_input>"), "{waiting}");
    assert!(waiting.contains("non-interactive"), "{waiting}");
    assert!(waiting.contains("nicht-interaktive"), "{waiting}");
    assert!(waiting.contains("<cancelled_by_user>"));
    let plain = format_command_result("apt install x", &output, true, false);
    assert!(!plain.contains("<waiting_for_input>"));
    let finished = format_command_result("apt install x", &output, false, true);
    assert!(!finished.contains("<waiting_for_input>"));
}
