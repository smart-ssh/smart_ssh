//! Issue #609 (Spec 0006): parsing of provider-reported token usage into
//! [`AiUsage`]. Only numeric/metadata fields are read — never message
//! content — and nothing is guessed: a field the endpoint did not send
//! stays `None`.

use serde_json::Value;
use ssh_manager_core::ai::AiUsage;

fn u64_at(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

fn str_at(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Merge an Anthropic `usage` object (from `message_start` or the final
/// `message_delta`) into `acc`. Fields present in `usage` overwrite earlier
/// values (the counters are cumulative); absent fields keep what is there.
pub(crate) fn merge_anthropic(acc: &mut AiUsage, usage: &Value) {
    if let Some(n) = u64_at(usage, "input_tokens") {
        acc.input_tokens = Some(n);
    }
    if let Some(n) = u64_at(usage, "output_tokens") {
        acc.output_tokens = Some(n);
    }
    if let Some(n) = u64_at(usage, "cache_read_input_tokens") {
        acc.cache_read_tokens = Some(n);
    }
    // Prefer the per-TTL breakdown (sum), fall back to the flat total.
    let breakdown = usage
        .get("cache_creation")
        .and_then(Value::as_object)
        .map(|o| {
            o.values()
                .filter_map(Value::as_u64)
                .fold(0u64, u64::saturating_add)
        })
        .filter(|_| {
            usage
                .get("cache_creation")
                .and_then(Value::as_object)
                .is_some_and(|o| o.values().any(Value::is_u64))
        });
    if let Some(n) = breakdown.or_else(|| u64_at(usage, "cache_creation_input_tokens")) {
        acc.cache_write_tokens = Some(n);
    }
    if let Some(n) = usage
        .get("server_tool_use")
        .and_then(|s| u64_at(s, "web_search_requests"))
    {
        acc.web_search_requests = Some(u32::try_from(n).unwrap_or(u32::MAX));
    }
    if let Some(t) = str_at(usage, "service_tier") {
        acc.service_tier = Some(t);
    }
}

/// Parse a Chat Completions `usage` object (OpenAI, OpenRouter, Ollama, …).
pub(crate) fn parse_chat_completions(usage: &Value) -> AiUsage {
    let prompt_details = usage.get("prompt_tokens_details");
    let completion_details = usage.get("completion_tokens_details");
    AiUsage {
        input_tokens: u64_at(usage, "prompt_tokens"),
        output_tokens: u64_at(usage, "completion_tokens"),
        cache_read_tokens: prompt_details.and_then(|d| u64_at(d, "cached_tokens")),
        cache_write_tokens: prompt_details.and_then(|d| u64_at(d, "cache_write_tokens")),
        reasoning_tokens: completion_details.and_then(|d| u64_at(d, "reasoning_tokens")),
        web_search_requests: None,
        service_tier: None,
        response_model: None,
        provider_reported_cost_usd: usage
            .get("cost")
            .and_then(Value::as_f64)
            .filter(|c| c.is_finite() && *c >= 0.0),
    }
}

/// Parse a Responses API `usage` object.
pub(crate) fn parse_responses(usage: &Value) -> AiUsage {
    AiUsage {
        input_tokens: u64_at(usage, "input_tokens"),
        output_tokens: u64_at(usage, "output_tokens"),
        cache_read_tokens: usage
            .get("input_tokens_details")
            .and_then(|d| u64_at(d, "cached_tokens")),
        reasoning_tokens: usage
            .get("output_tokens_details")
            .and_then(|d| u64_at(d, "reasoning_tokens")),
        ..AiUsage::default()
    }
}

pub(crate) fn model_of(v: &Value) -> Option<String> {
    str_at(v, "model")
}

pub(crate) fn service_tier_of(v: &Value) -> Option<String> {
    str_at(v, "service_tier")
}
