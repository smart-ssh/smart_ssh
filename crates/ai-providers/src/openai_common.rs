//! Code shared by the two OpenAI-family providers (`openai_compatible` for
//! Chat Completions, `openai_responses` for the Responses API): the model
//! max-token tables, the SSE stream state, the connect/attempt function and
//! the `stream_options` rejection cache.

use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;

use futures::Stream;
use serde_json::Value;
use uuid::Uuid;

use ssh_manager_core::ai::{AiError, AiEvent};

use crate::action::tool_call_event;
use crate::common::frame_loop::FrameHandler;
use crate::common::http_retry::{send_with_retry, SendLoopParams, SendOutcome};
use crate::common::max_tokens_retry::{raw_event_stream, to_raw_stream, RawEvent};
use crate::error::{error_stream, map_stream_error_object, provider_request_id, ProviderErrorInfo};
use crate::fallback::parse_fallback_response;
use crate::request_logging::{
    log_openai_round_summary, log_provider_transport_error, log_stop_reason, log_tool_call_fragment,
};
use crate::sse::SseFrame;
use ssh_manager_core::ai::AiUsage;

/// Fallback für jeden Endpunkt, der nicht nachweislich die offizielle
/// OpenAI-API ist (Spec 0065, Teil 1: "Nicht-Anthropic-Provider: Default
/// ebenfalls modellabhängig, aber VORSICHTIGER" — manche Gateways
/// reservieren anhand von `max_tokens` oder lehnen zu hohe Werte mit 400 ab,
/// ein selbstgehostetes/lokales Modell hat oft nur ein kleines
/// Output-Limit).
///
/// **Spec 0080, §8, Klarstellung (2026-09-24, P1 Variante b):** vor
/// diesem Wert galt hier 4096 (Default 2048) — beobachtet zu knapp für
/// Reasoning-Modelle an einem Nicht-OpenAI-Endpunkt, deren Denk-Tokens das
/// Budget aufbrauchen, bevor überhaupt Text entsteht (Spec 0080 §1). Jetzt
/// 16384 (Default 8192, s. [`openai_compatible_default_max_tokens`]), auf
/// Basis einer Recherche der Anbieter-Doku: harte Output-Obergrenzen liegen
/// bei den gängigen Anbietern bei 8192 oder höher. Ein HTTP 400 wegen "über
/// dem Limit" bleibt möglich (vor allem bei selbst gehosteten Servern mit
/// kleinem Kontext) — dafür existiert `max_tokens_override`
/// (Spec 0065, Teil 4).
///
/// Spec-reviewer-Fund (ERHÖHT, Review dieses Schritts): die Klarstellung
/// ist wörtlich auf "Endpunkte außer der offiziellen OpenAI-API" begrenzt
/// — dieser Wert wird deshalb NUR noch im frühen Rückgabe-Zweig für einen
/// Nicht-OpenAI-`base_url` verwendet. Der unabhängige Unbekannt-Fallback
/// FÜR die offizielle API (ein Modellname, der zu keinem der bekannten
/// Muster passt) bleibt [`OPENAI_COMPATIBLE_OFFICIAL_API_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS`]
/// unverändert bei 4096 — für den lässt Spec 0080 keine Anhebung zu, und
/// die offizielle API ist ohnehin die Umgebung, in der ein tatsächlich
/// zu enges Limit am ehesten dokumentiert (und damit vermeidbar) wäre.
pub(crate) const OPENAI_COMPATIBLE_NON_OPENAI_ENDPOINT_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS: u32 =
    16_384;

/// Unbekannter Modellname AN DER OFFIZIELLEN OpenAI-API (kein Muster unten
/// passt) — bewusst eine EIGENE, von
/// [`OPENAI_COMPATIBLE_NON_OPENAI_ENDPOINT_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS`]
/// getrennte Konstante seit Spec 0080 (s. deren Doc-Kommentar): vor Spec
/// 0080 teilten sich beide Fälle denselben Wert (4096), Spec 0080 §8 hebt
/// aber ausdrücklich nur den Nicht-OpenAI-Fall an.
pub(crate) const OPENAI_COMPATIBLE_OFFICIAL_API_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS: u32 = 4_096;

/// Modellabhängiges Output-Maximum (Spec 0065, Teil 1) — nur für die
/// offizielle OpenAI-API angewendet (erkannt an `base_url`), da `model` bei
/// einem generischen Gateway/Ollama frei wählbar ist und dessen
/// tatsächliches Output-Maximum von hier aus grundsätzlich nicht bekannt
/// sein kann. Verifiziert gegen developers.openai.com/api/docs/models
/// (Stand dieser Implementierung, 2026-09): die aktuelle Flaggschiff-
/// Generation (u. a. `gpt-5.6-*`, `gpt-6-*`) hat durchweg 128K Max-Output;
/// `gpt-3.5`/klassisches `gpt-4`(-turbo)/`gpt-4o`/`gpt-4.1` sind kleiner,
/// namentlich bekannte Ausnahmen mit ihrem jeweils dokumentierten Wert.
///
/// Spec-reviewer-Fund (ERHÖHT, Review dieses Schritts): die Fallback-
/// Richtung war invertiert — ein UNBEKANNTER Modellname (klassisches
/// `gpt-4`, `o1`, `o3`, jedes künftige Namensschema) landete im `else`-
/// Zweig und bekam den GRÖSSTEN Wert (128K) statt des in Spec 0065 §1
/// verlangten konservativen Fallbacks ("kein 400 wegen ‚über dem
/// Maximum'"). Jetzt umgekehrt: nur EXPLIZIT als aktuelle Generation
/// bekannte Namen bekommen 128K, alles andere (inkl. unbekannt) fällt auf
/// den konservativen Wert zurück.
pub(crate) fn openai_compatible_model_max_output_tokens(base_url: &str, model: &str) -> u32 {
    if !base_url.contains("api.openai.com") {
        return OPENAI_COMPATIBLE_NON_OPENAI_ENDPOINT_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS;
    }
    let model = model.to_lowercase();
    if model.contains("gpt-3.5") {
        4_096
    } else if model.contains("gpt-4o") || model.contains("gpt-4-turbo") || model.contains("gpt-4.1")
    {
        16_384
    } else if model.starts_with("gpt-4") {
        // Klassisches gpt-4/gpt-4-0613 u. ä. — NICHT gpt-4o/-turbo/-4.1
        // (oben bereits behandelt) — deutlich kleiner als die aktuelle
        // Generation.
        8_192
    } else if model.starts_with("o1-mini") {
        65_536
    } else if model.starts_with("o1") {
        100_000
    } else if model.contains("gpt-5") || model.contains("gpt-6") || model.starts_with("o3") {
        // Aktuelle Flaggschiff-/Reasoning-Generation — 128K verifiziert
        // (s. Funktionsdoc); o3 mangels eigener verifizierter Doku-Zeile
        // konservativ in dieselbe Gruppe wie die übrige aktuelle Generation
        // eingeordnet statt in den generellen Unbekannt-Fallback.
        128_000
    } else {
        OPENAI_COMPATIBLE_OFFICIAL_API_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS
    }
}

/// Standardwert für den Haupt-Chat (Spec 0065, Teil 1) — analog zu
/// `crate::anthropic::anthropic_default_max_tokens`s Begründung: BEWUSST
/// kleiner als das Modell-Maximum, sonst hat der einmalige Retry aus Teil 3
/// keinen Spielraum zum Verdoppeln mehr (spec-reviewer-Fund, ERHÖHT, Review
/// dieses Schritts — beide waren in einer früheren Fassung identisch).
pub(crate) fn openai_compatible_default_max_tokens(base_url: &str, model: &str) -> u32 {
    let max = openai_compatible_model_max_output_tokens(base_url, model);
    match max {
        128_000 => 32_000,
        _ => (max / 2).max(1),
    }
}

/// Spec-reviewer-Fund (ERHÖHT, Review dieses Schritts): die OpenAI-
/// Reasoning-Modelle (o1/o3/o4-Serie, gpt-5.x) lehnen das klassische
/// `max_tokens`-Feld mit einem 400 ab und verlangen stattdessen
/// `max_completion_tokens` — dieser Provider setzte vor Spec 0065
/// überhaupt kein `max_tokens`, das ist also eine echte Regression, die
/// dieser Fix schließt. Nur für die offizielle OpenAI-API ausgewertet
/// (`model` ist bei einem generischen Gateway frei wählbar und ein
/// Gateway kann denselben Modellnamen unter dem klassischen Feld
/// erwarten).
pub(crate) fn openai_max_tokens_field_name(base_url: &str, model: &str) -> &'static str {
    let model = model.to_lowercase();
    if base_url.contains("api.openai.com")
        && (model.starts_with("o1")
            || model.starts_with("o3")
            || model.starts_with("o4")
            || model.contains("gpt-5"))
    {
        "max_completion_tokens"
    } else {
        "max_tokens"
    }
}

/// Spec 0087, A1.2: fest gewähltes Retry-Budget statt aus dem Fehlertext
/// gerechneter Zahlen (§5: drei Textformate mit Zahlen in Prosa wären
/// fragil, Halbieren scheitert bei genau 4k-Kontext weiterhin). Neben den
/// übrigen Budget-Konstanten dieses Providers.
pub(crate) const OPENAI_COMPATIBLE_CONTEXT_LIMIT_RETRY_MAX_TOKENS: u32 = 2048;

/// Spec 0087, A1.1: Textbausteine (case-insensitive), die auf einen HTTP-400
/// wegen Überschreiten der Kontextgrenze hindeuten — belegt gegen vLLM
/// (Quellcode) und OpenRouter (zitierter Fehlerbericht), s. Spec Abschnitt 1.
/// Bewusst NICHT llama.cpps `"exceeds the available context size"` (Spec
/// Abschnitt 1, Nicht-Ziele: llama.cpp prüft nur die Eingabe, ein kleineres
/// Budget hilft dort nicht — T3 ist der Negativ-Test dafür).
const CONTEXT_LENGTH_ERROR_MARKERS: &[&str] = &[
    "maximum context length is",
    "cannot be greater than max_model_len",
];

/// Spec 0087, A1.1: wirkt NUR hier (OpenAI-kompatibler Provider), nicht in
/// der mit `AnthropicProvider` geteilten `crate::error::map_http_status` —
/// T11b belegt, dass ein identischer Anthropic-400-Body unverändert
/// `ProviderUnavailable` bleibt.
pub(crate) fn is_context_length_error(body: &str) -> bool {
    let lower = body.to_lowercase();
    CONTEXT_LENGTH_ERROR_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

/// Turns a successful HTTP response into the raw event stream — one function
/// per API format (Chat Completions, Responses).
pub(crate) type StreamParser = fn(
    reqwest::Response,
    bool,
    Uuid,
    String,
    Vec<(String, String)>,
    Option<String>,
) -> Pin<Box<dyn Stream<Item = RawEvent> + Send>>;

/// Issue #609: endpoint+model keys that rejected `stream_options` with a 400.
/// Later requests to them omit the option up front. Owned by the provider
/// registry (`RateLimitRegistry`), so the memory lasts as long as the
/// registry, which lives once per app run; every provider built from the
/// same registry shares it, one from another registry starts empty.
#[derive(Debug, Clone, Default)]
pub struct StreamOptionsRejections {
    keys: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl StreamOptionsRejections {
    pub fn new() -> Self {
        Self::default()
    }

    fn contains(&self, key: &str) -> bool {
        self.keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(key)
    }

    fn insert(&self, key: String) {
        self.keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key);
    }
}

/// s. `crate::anthropic::connect_and_stream`-Kommentar — identisches Muster,
/// losgelöst von `send()`, damit Spec 0065 Teil 3 sie ein zweites Mal mit
/// höherem `max_tokens` aufrufen kann.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn connect_and_stream(
    parse: StreamParser,
    client: reqwest::Client,
    url: String,
    api_key: String,
    native_tool_calling: bool,
    request_id: Uuid,
    extra_headers: Vec<(String, String)>,
    stream_options_rejections: StreamOptionsRejections,
    mut body: Value,
) -> Pin<Box<dyn Stream<Item = RawEvent> + Send>> {
    // Issue #609: `stream_options` (usage reporting) is optional. Drop it up
    // front for an endpoint that rejected it earlier in this run.
    let mut dropped_stream_options = false;
    let stream_options_key = format!(
        "{url}\n{}",
        body.get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
    );
    if body.get("stream_options").is_some()
        && stream_options_rejections.contains(&stream_options_key)
    {
        body.as_object_mut().map(|o| o.remove("stream_options"));
        dropped_stream_options = true;
    }
    // Spec-Reviewer-Fund (Spec 0049, Review von Fund 2): nicht nur der
    // API-Key, auch jeder `extra_headers`-Wert (Spec 0025, Abschnitt 3 —
    // dort trägt ein Nutzer z. B. ein zweites Gateway-Auth-Token ein) muss
    // in den neuen Fehler-Logzeilen redigiert werden.
    let secrets: Vec<&str> = std::iter::once(api_key.as_str())
        .chain(extra_headers.iter().map(|(_, value)| value.as_str()))
        .collect();

    // One shared attempt loop (issue #851), see `common::http_retry`.
    let response = loop {
        let outcome = send_with_retry(
            || {
                let mut req = client
                    .post(&url)
                    .bearer_auth(&api_key)
                    .header("accept", "text/event-stream");
                for (name, value) in &extra_headers {
                    req = req.header(name, value);
                }
                req.json(&body)
            },
            SendLoopParams {
                request_id,
                secrets: &secrets,
                on_headers: None,
            },
        )
        .await;
        let rejected = match outcome {
            Ok(SendOutcome::Success(response)) => break response,
            Ok(SendOutcome::Rejected(rejected)) => rejected,
            Err(mapped) => return to_raw_stream(error_stream(mapped)),
        };
        let status = rejected.status;
        let text = &rejected.text;
        let mapped = rejected.map();
        // Issue #609: an endpoint answering 400 while `stream_options` is on
        // the request is retried once without it — nothing else in the
        // request changes. Remembered only once the retry succeeds.
        if status.as_u16() == 400
            && body.get("stream_options").is_some()
            && !matches!(mapped, AiError::ModelNotFound(_))
            && !is_context_length_error(text)
            && !text.to_lowercase().contains("context")
        {
            tracing::info!(
                request_id = %request_id,
                "endpoint rejected stream_options; retrying once without usage reporting",
            );
            body.as_object_mut().map(|o| o.remove("stream_options"));
            dropped_stream_options = true;
            continue;
        }
        // Spec 0049, Fund 2 / Spec 0087, A1.6: logged here, also when the
        // branch below returns `ContextLimitError` instead of an error stream.
        rejected.log(request_id, &mapped, &secrets);
        // Spec 0087, A1.1: a 400 not already recognised as "model not found"
        // whose body carries a context-length marker (this provider only).
        if status.as_u16() == 400
            && matches!(
                mapped,
                AiError::ProviderUnavailable(_)
                    | AiError::RejectedRequest(_)
                    | AiError::ContextTooLarge
            )
            && is_context_length_error(text)
        {
            return raw_event_stream(RawEvent::ContextLimitError);
        }
        return to_raw_stream(error_stream(mapped));
    };

    if dropped_stream_options {
        stream_options_rejections.insert(stream_options_key.clone());
    }
    let provider_rid = provider_request_id(response.headers());
    parse(
        response,
        native_tool_calling,
        request_id,
        api_key,
        extra_headers,
        provider_rid,
    )
}

/// Akkumulator für ein einzelnes, über mehrere Chunks verteiltes
/// Tool-Call-Fragment (`name`/`arguments` werden jeweils als Teilstrings
/// geliefert und müssen aneinandergehängt werden).
#[derive(Default)]
pub(crate) struct ToolCallAccumulator {
    pub(crate) name: String,
    pub(crate) arguments: String,
}

pub(crate) struct OpenAiStreamState {
    pub(crate) tool_calls: BTreeMap<u64, ToolCallAccumulator>,
    pub(crate) fallback_text: String,
    /// Spec 0049, Fund 2: API-Key + jeder `extra_headers`-Wert, für die
    /// Redaction bei einem Transport-Fehler mitten im Stream (s.
    /// `AnthropicStreamState::api_key`-Doc-Kommentar — derselbe Grund).
    /// Eigene, besitzende `String`s statt `&str`, da sie über die gesamte
    /// Stream-Laufzeit gebraucht werden, nicht nur innerhalb der
    /// `async move`-Anfrage, aus der `api_key`/`extra_headers` stammen.
    pub(crate) secrets: Vec<String>,
    /// s. `AnthropicStreamState::text_delta_total_len` (Spec 0016,
    /// Abschnitt 4, Punkt 2).
    pub(crate) text_delta_total_len: usize,
    /// Spec 0080, A4: Gesamtlänge der Denk-Deltas (`delta.reasoning_content`
    /// bzw. `delta.reasoning`, je nach Gateway) — nur gezählt, s.
    /// `handle_chunk`. Nie in `fallback_text` oder als `TextDelta`
    /// weitergegeben (Spec 0080 §4, Invariante 1).
    pub(crate) reasoning_delta_total_len: usize,
    pub(crate) native_tool_calling: bool,
    pub(crate) pending: VecDeque<RawEvent>,
    pub(crate) finished: bool,
    pub(crate) request_id: Uuid,
    /// Issue #599: the provider's own request id (response header), for the
    /// in-stream error log line.
    pub(crate) provider_request_id: Option<String>,
    /// Spec 0065, Teil 3: letzter gesehener `finish_reason` — `None`, wenn
    /// noch keiner ankam (z. B. bei einem abrupten Verbindungsabbruch vor
    /// jedem Chunk mit diesem Feld).
    pub(crate) finish_reason: Option<String>,
    /// Issue #609: provider-reported usage, delivered once by `finalize`.
    pub(crate) usage: AiUsage,
}

impl OpenAiStreamState {
    /// Issue #599: maps an in-stream error to an [`AiError`] and writes the
    /// one WARN line for it. `info` is what the provider sent.
    pub(crate) fn report_stream_error(&self, info: &ProviderErrorInfo, mapped: &AiError) {
        let secrets: Vec<&str> = self.secrets.iter().map(String::as_str).collect();
        crate::request_logging::log_provider_error(
            self.request_id,
            self.provider_request_id.as_deref(),
            None,
            info.error_type.as_deref().or(info.code.as_deref()),
            mapped,
            info.message.as_deref().unwrap_or("unbekannter Fehler"),
            &secrets,
        );
    }

    /// Issue #609: the usage as a public event, at most once.
    /// Issue #617: ahead of it, the real input size as [`AiEvent::Usage`]
    /// (chat compaction / context indicator). OpenAI-style APIs count cached
    /// tokens inside the prompt tokens, so `input_tokens` is already the
    /// full prompt size and the cache fields are not added again.
    pub(crate) fn take_usage_events(&mut self) -> Vec<RawEvent> {
        let mut events = Vec::new();
        if let Some(total) = self.usage.input_tokens.filter(|t| *t > 0) {
            events.push(RawEvent::Public(AiEvent::Usage {
                input_tokens: total,
            }));
        }
        events.extend(self.take_usage_event());
        events
    }

    pub(crate) fn take_usage_event(&mut self) -> Option<RawEvent> {
        let usage = std::mem::take(&mut self.usage);
        // A bare model name without any count is not a usage report.
        let reported = usage.input_tokens.is_some()
            || usage.output_tokens.is_some()
            || usage.cache_read_tokens.is_some()
            || usage.cache_write_tokens.is_some()
            || usage.reasoning_tokens.is_some()
            || usage.provider_reported_cost_usd.is_some();
        reported.then_some(RawEvent::Public(AiEvent::RequestUsage(usage)))
    }

    pub(crate) fn handle_chunk(&mut self, chunk: &Value) {
        // Issue #599: an in-stream `{"error": {...}}` chunk ends the response
        // with a classified error; a partial tool call is never released.
        if let Some(error) = chunk.get("error").filter(|e| !e.is_null()) {
            let (mapped, info) = map_stream_error_object(error);
            self.report_stream_error(&info, &mapped);
            self.tool_calls.clear();
            self.pending
                .push_back(RawEvent::Public(AiEvent::Error(mapped)));
            self.finished = true;
            return;
        }
        let choice = chunk.get("choices").and_then(|choices| choices.get(0));

        // Issue #609: the final usage chunk (`stream_options.include_usage`;
        // OpenRouter and Ollama also send it unconditionally). Numeric and
        // metadata fields only.
        if let Some(usage) = chunk.get("usage").filter(|u| u.is_object()) {
            let model = self.usage.response_model.take();
            self.usage = crate::usage::parse_chat_completions(usage);
            self.usage.response_model = model;
        }
        if let Some(model) = crate::usage::model_of(chunk) {
            self.usage.response_model = Some(model);
        }
        if let Some(tier) = crate::usage::service_tier_of(chunk) {
            self.usage.service_tier = Some(tier);
        }

        // analog zu Anthropics `stop_reason` (s.
        // `AnthropicStreamState::handle_event`s `message_delta`-Zweig) —
        // `finish_reason` steht auf demselben `choices[0]`-Objekt wie
        // `delta`, meist im letzten Chunk mit leerem/fehlendem `delta`, also
        // hier unabhängig vom `delta`-Fetch unten geprüft statt danach.
        if let Some(finish_reason) = choice
            .and_then(|c| c.get("finish_reason"))
            .and_then(Value::as_str)
        {
            log_stop_reason(self.request_id, "openai_compatible", finish_reason);
            // Spec 0065, Teil 3: gespeichert, damit `finalize()` weiß, ob
            // akkumulierte Tool-Call-Fragmente freigegeben werden dürfen.
            self.finish_reason = Some(finish_reason.to_string());
        }

        let Some(delta) = choice.and_then(|c| c.get("delta")) else {
            return;
        };

        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                self.text_delta_total_len += content.len();
                if self.native_tool_calling {
                    self.pending
                        .push_back(RawEvent::Public(AiEvent::TextDelta(content.to_string())));
                } else {
                    self.fallback_text.push_str(content);
                }
            }
        }

        // Spec 0080, A4: Denk-Deltas nur zählen — NIE an `fallback_text`
        // anhängen (sonst könnte ein zufällig aktionsblock-förmiger
        // Denkinhalt vom Fallback-Aktions-Parser gelesen werden, s. T9) und
        // NIE als `TextDelta` weiterreichen (Invariante 1, Spec 0080 §4).
        // Feldname je Gateway unterschiedlich (`reasoning_content` z. B.
        // DeepSeek-artige Konvention, `reasoning` andere Anbieter) — beide
        // geprüft, `reasoning_content` zuerst (kein bekannter Gateway
        // liefert je beide gleichzeitig, die Reihenfolge ist also nur zur
        // Eindeutigkeit dieser Implementierung, keine echte Priorisierung).
        let reasoning_delta = delta
            .get("reasoning_content")
            .and_then(Value::as_str)
            .or_else(|| delta.get("reasoning").and_then(Value::as_str));
        if let Some(reasoning) = reasoning_delta {
            self.reasoning_delta_total_len += reasoning.len();
        }

        if !self.native_tool_calling {
            return;
        }
        let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) else {
            return;
        };
        for tool_call in tool_calls {
            let index = tool_call.get("index").and_then(Value::as_u64).unwrap_or(0);
            let entry = self.tool_calls.entry(index).or_default();
            if let Some(name) = tool_call
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
            {
                entry.name.push_str(name);
            }
            if let Some(arguments) = tool_call
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
            {
                entry.arguments.push_str(arguments);
            }
        }
    }

    /// Spec 0065, Teil 3 (sicherheitskritisch): `abrupt` ist `true`, wenn der
    /// Stream ohne das `[DONE]`-Literal endete (Verbindungsabbruch) — dann
    /// ist ein ggf. akkumulierter Tool-Call per Definition unvollständig,
    /// UNABHÄNGIG von `finish_reason` (der in diesem Fall oft gar nicht
    /// mehr ankam). Der Ist-Befund vor diesem Fix: `finalize()` versuchte
    /// in JEDEM Fall (auch `abrupt`), akkumulierte `tool_calls` zu parsen —
    /// ganz ohne Rücksicht auf `finish_reason`/den Verbindungszustand.
    pub(crate) fn finalize(&mut self, abrupt: bool) -> Vec<RawEvent> {
        // Spec 0080, A4: eigene, IMMER (auch bei `text_len == 0`) loggende
        // Funktion statt der geteilten `log_text_delta_summary` — die
        // überspringt bei 0 ganz (für Anthropic unverändert gewollt, s.
        // `crate::anthropic::AnthropicStreamState::finalize`, Spec 0080 §2:
        // "keine Änderungen am Anthropic-Provider"), aber genau der
        // `text_len == 0`-Fall ist hier der interessante, den Spec 0080
        // messbar machen will (die leere, abgeschnittene Runde aus §1).
        log_openai_round_summary(
            self.request_id,
            self.text_delta_total_len,
            self.reasoning_delta_total_len,
        );
        // Spec 0065, Teil 2: `TextTruncated` (UI-Hinweis + „Weiter") gilt nur
        // für ein echtes `finish_reason: length` — ein abrupter
        // Verbindungsabbruch (`abrupt`) ist ein anderer Fehlerfall (bereits
        // separat als `AiError::NetworkError` sichtbar), keine "Antwort war
        // zu lang"-Situation.
        let finish_reason_is_length = self.finish_reason.as_deref() == Some("length");
        // Spec-reviewer-Fund (ERHÖHT, Review dieses Schritts): ALLOWLIST
        // statt Denylist für die Sicherheits-Entscheidung (Tool-Call
        // freigeben oder verwerfen) — analog zu `crate::anthropic`s
        // identischem Fix. Ein Gateway/Proxy, das `finish_reason` anders
        // schreibt (`"Length"`, `"MAX_TOKENS"`) oder gar keins liefert,
        // hätte mit der alten `== "length"`-Prüfung einen abgeschnittenen
        // Tool-Call fälschlich als vollständig durchgelassen. Nur `"stop"`/
        // `"tool_calls"` (OpenAIs dokumentierte Erfolgs-Werte) gelten als
        // vollständig — `finish_reason_is_length` bleibt separat für den
        // rein informativen `TextTruncated`-UI-Hinweis (kein
        // Sicherheits-Gate).
        let finish_reason_confirms_completion = matches!(
            self.finish_reason.as_deref(),
            Some("stop") | Some("tool_calls")
        );
        let tool_call_truncated = abrupt || !finish_reason_confirms_completion;

        // Issue #647: `content_filter` is a refusal — own outcome, no retry,
        // pending tool calls are dropped (never released).
        if self.finish_reason.as_deref() == Some("content_filter") {
            self.tool_calls.clear();
            let mut events = Vec::new();
            if !self.native_tool_calling {
                let result = parse_fallback_response(&self.fallback_text);
                if !result.text.is_empty() {
                    events.push(RawEvent::Public(AiEvent::TextDelta(result.text)));
                }
            }
            if let Some(event) = self.take_usage_event() {
                events.push(event);
            }
            events.push(RawEvent::Public(AiEvent::Refused));
            return events;
        }

        if self.native_tool_calling {
            if tool_call_truncated && !self.tool_calls.is_empty() {
                // Spec 0065 §3, konservative Multi-Tool-Regel: die GANZE
                // Antwort verwerfen, nicht nur den zuletzt akkumulierten
                // Call — OpenAI-kompatible Antworten liefern keine
                // Block-für-Block-Abschlussgrenze wie Anthropics
                // `content_block_stop`, es lässt sich also nicht
                // unterscheiden, welcher der akkumulierten Calls
                // tatsächlich vollständig war.
                self.tool_calls.clear();
                return vec![RawEvent::RetryWithHigherMaxTokens];
            }
            // Spec 0080, A1: die leere, abgeschnittene Runde — nur bei
            // echtem `finish_reason: "length"`, OHNE jeden Textinhalt und
            // OHNE jeden Tool-Call — löst denselben (bestehenden) Retry aus
            // statt in `TextTruncated` zu enden (§1: bei einem
            // Reasoning-Modell ohne `max_tokens_override` verbraucht die
            // Denkphase sonst das ganze Budget, die Antwort bleibt für den
            // Nutzer unsichtbar). `self.tool_calls.is_empty()` ist an dieser
            // Stelle bereits durch das `if` oben impliziert (wäre es nicht
            // leer UND `tool_call_truncated`, wäre schon dort
            // zurückgekehrt) — trotzdem explizit geprüft: die bestehende
            // Tool-Call-Prüfung bleibt unverändert stehen, diese neue
            // Bedingung kommt nur ODER-verknüpft hinzu (nie eine bestehende
            // Prüfung ersetzen, s. CLAUDE.md "Sicherheitsänderungen").
            if finish_reason_is_length
                && self.text_delta_total_len == 0
                && self.tool_calls.is_empty()
            {
                return vec![RawEvent::RetryWithHigherMaxTokens];
            }
            let mut events = Vec::new();
            for (_, call) in std::mem::take(&mut self.tool_calls) {
                log_tool_call_fragment(self.request_id, &call.name, &call.arguments);
                events.push(RawEvent::Public(
                    // Issue #604: unknown tool, broken JSON or invalid fields are
                    // non-fatal (`AiEvent::InvalidToolCall`).
                    tool_call_event(self.request_id, &call.name, &call.arguments),
                ));
            }
            events.extend(self.take_usage_events());
            events.push(RawEvent::Public(if finish_reason_is_length {
                AiEvent::TextTruncated
            } else {
                AiEvent::Done
            }));
            events
        } else {
            let result = parse_fallback_response(&self.fallback_text);
            if tool_call_truncated && result.action.is_some() {
                return vec![RawEvent::RetryWithHigherMaxTokens];
            }
            // Spec 0080, A1 — Fallback-Pendant zum nativen Zweig oben:
            // dieselbe leere Runde, hier ohne erkannte Aktion statt ohne
            // Tool-Call. `result.action.is_none()` ist an dieser Stelle
            // bereits durch das `if` oben impliziert, s. dortiger
            // Kommentar zur selben expliziten Redundanz.
            if finish_reason_is_length && self.text_delta_total_len == 0 && result.action.is_none()
            {
                return vec![RawEvent::RetryWithHigherMaxTokens];
            }
            let mut events = Vec::new();
            if !result.text.is_empty() {
                events.push(RawEvent::Public(AiEvent::TextDelta(result.text)));
            }
            if let Some(action) = result.action {
                events.push(RawEvent::Public(AiEvent::ActionProposed(action)));
            }
            events.extend(self.take_usage_events());
            events.push(RawEvent::Public(if finish_reason_is_length {
                AiEvent::TextTruncated
            } else {
                AiEvent::Done
            }));
            events
        }
    }
}

impl OpenAiStreamState {
    /// Shared by Chat Completions and Responses: both stream into this state.
    pub(crate) fn new(
        native_tool_calling: bool,
        request_id: Uuid,
        api_key: String,
        extra_headers: Vec<(String, String)>,
        provider_request_id: Option<String>,
    ) -> Self {
        let secrets: Vec<String> = std::iter::once(api_key)
            .chain(extra_headers.into_iter().map(|(_, value)| value))
            .collect();
        Self {
            tool_calls: BTreeMap::new(),
            fallback_text: String::new(),
            secrets,
            text_delta_total_len: 0,
            reasoning_delta_total_len: 0,
            native_tool_calling,
            pending: VecDeque::new(),
            finished: false,
            request_id,
            provider_request_id,
            finish_reason: None,
            usage: AiUsage::default(),
        }
    }

    /// Redacted logging of a transport/timeout error (Spec 0049, finding 2).
    pub(crate) fn log_transport_error_redacted(&self, error: &AiError) {
        let secrets: Vec<&str> = self.secrets.iter().map(String::as_str).collect();
        log_provider_transport_error(self.request_id, error, &secrets);
    }
}

impl FrameHandler for OpenAiStreamState {
    type Event = RawEvent;

    fn on_frame(&mut self, frame: &SseFrame) -> Vec<RawEvent> {
        if frame.data.trim() == "[DONE]" {
            self.finished = true;
            let events = self.finalize(false);
            self.pending.extend(events);
        } else if let Ok(chunk) = serde_json::from_str::<Value>(&frame.data) {
            self.handle_chunk(&chunk);
        }
        // Unparsable frames are ignored instead of aborting the stream: one
        // broken chunk must not make the whole response unusable.
        self.pending.drain(..).collect()
    }

    fn is_finished(&self) -> bool {
        self.finished
    }

    /// Spec 0065, part 3: `abrupt = true` means no `[DONE]` was seen.
    fn finalize(&mut self, abrupt: bool) -> Vec<RawEvent> {
        OpenAiStreamState::finalize(self, abrupt)
    }

    fn log_transport_error(&self, error: &AiError) {
        self.log_transport_error_redacted(error);
    }
}
