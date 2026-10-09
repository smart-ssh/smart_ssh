# ADR 0127 — Web research for the official OpenAI provider

Status: accepted
Concerns: Issue #168 (part 2), Spec 0105, ADR 0117 (decision 1 superseded for
the official OpenAI provider), ADR 0126

## Problem

ADR 0117 limited web research to Anthropic. Chat Completions search models
always search, so they cannot honour the "Web research" switch (Spec 0105
§2). With the official provider on the Responses API (ADR 0126), the
`web_search` tool is available and the model decides whether to search.

## Decision

1. **Scope.** Only the official OpenAI provider (ADR 0126, point 1) sends the
   `web_search` tool. Custom base URLs, compatible endpoints and Ollama never
   get a web field.
2. **Request.** The tool is sent only when the switch is on, only for the
   main chat (no `max_tokens_hint`), only with native tool calling, and
   always first in the tool list. `max_tool_calls: 5` bounds the searches.
   With the switch off no web field is sent.
3. **Mapping.** Each `web_search_call` output item (action `search`)
   becomes one `Search` activity with its query; a `failed` status sets an
   error code. `url_citation` annotations (stream annotation events and
   finished message items, deduplicated) become the cited sources.
   Activities are held to the end of the response and dropped together with
   a discarded (retried) or failed response.
4. **No page reads.** The Responses tool returns no page text, so
   `open_page`/`find_in_page` actions are not shown and nothing is stored or
   fenced as page content.
5. **Citations and injection check.** Citations cannot be tied to one search;
   they are attached to the last search of the response. They are stored as
   both `results` and `cited`, so the existing injection check (titles of
   `results`) covers them. Redaction, fencing and the "untrusted content
   ingested" flag are the shared code and unchanged.
6. **Rejected tool.** The account-level rejection of Anthropic's tools
   (ADR 0124) is documented for Anthropic only; OpenAI errors surface as
   ordinary provider errors.
7. **Default.** The setting defaults to on for all providers, so existing
   OpenAI providers start with web research after the update (Spec 0105 §2).

## Consequences

- Web research results of OpenAI are less detailed than Anthropic's (no hit
  list, no page text).
- The OpenAI request and the stored history carry a new activity kind only
  as the existing `WebActivity`; no schema change.
