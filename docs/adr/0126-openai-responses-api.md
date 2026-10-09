# ADR 0126 — Official OpenAI provider uses the Responses API

Status: accepted
Concerns: Issue #168 (part 1), Spec 0006, ADR 0117

## Problem

Web research for OpenAI (issue #168) needs the Responses API: Chat
Completions offers web search only on dedicated search models that always
search, which cannot honour the "Web research" switch (Spec 0105 §2). To keep
the transport change reviewable apart from the security-relevant web-content
handling, the main chat of the official OpenAI provider moves first, without
any visible change.

## Decision

1. **Identification.** "Official OpenAI" is provider type `openai` with no
   base URL or the default `https://api.openai.com/v1` (a trailing slash is
   ignored). Everything else (custom base URL, generic OpenAI-compatible
   endpoints, Ollama) keeps the Chat Completions request unchanged.
2. **Separate implementation.** The Responses path lives in its own
   provider next to the Chat Completions one. It shares the message
   formatting, the stream accumulator, the retry wrapper (429 retry,
   truncation retry, context-limit retry), error mapping and request logging.
3. **Stateless.** Every request sends `store: false` and never a
   `previous_response_id`; the full history is sent each time (ADR 0117,
   point 8).
4. **Tools.** Native tools are sent as Responses function tools with
   `strict: false`, matching the non-strict schemas used so far.
5. **Safety mapping.** The terminal event maps onto the existing completion
   allowlist: `response.completed` confirms completion,
   `response.incomplete` (`max_output_tokens`) counts as truncated, any
   other ending (no terminal event, other incomplete reason) discards
   accumulated tool calls. `response.failed` and `error` events surface as
   errors and never release a tool call.
6. **Output limit.** `max_output_tokens` carries the same value as before
   (hint, then override, then model default).

## Consequences

- No visible behaviour change; no spec change. Token usage is not
  evaluated, as before.
- Part 2 of #168 adds the `web_search` tool on this path.
