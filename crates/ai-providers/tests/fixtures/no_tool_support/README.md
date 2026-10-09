# Fixtures: model without tool-calling support (issue #99)

| File | Status | Date |
|---|---|---|
| `ollama.json` | **measured** — `POST /v1/chat/completions` with `tools` against Ollama 0.40.2, model `tinyllama:latest` | 2026-10-09 |

Controls from the same measurement: the model without `tools` → HTTP 200; a
tool-capable model with the same `tools` → HTTP 200; model not found → HTTP 404
with `"type":"not_found_error"`. No structured code exists (`code` is null), so
detection uses HTTP 400 + `error.type == "invalid_request_error"` + a message
ending in `does not support tools`. No secrets in the file.
