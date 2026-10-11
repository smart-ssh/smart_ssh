# Fixtures: „Modell nicht gefunden“ (Spec 0072, A4)

| Datei | Status | Datum |
|---|---|---|
| `anthropic.json` | **gemessen** — echter Aufruf gegen `POST /v1/messages` mit falschem Modellnamen | 2026-09-23 |
| `openai.json` | **gemessen** — `POST …/chat/completions` (`stream` true und false identisch), HTTP 404 | 2026-10-09 |
| `ollama.json` | **gemessen** — Ollama 0.40.2, lokal, HTTP 404 | 2026-10-09 |
| `openrouter.json` | **gemessen** — HTTP 400; kein strukturiertes Feld (`code` ist nur der HTTP-Status), erkannt über den Marker `is not a valid model id`. Der Antwort-Body trug ein `user_id`, das hier entfernt ist | 2026-10-09 |

`anthropic.json` trägt die tatsächliche Antwort (BL-0200, Messung
2026-09-23): `{"type":"error","error":{"type":"not_found_error","message":
"model: …"}}`. Die zuvor rekonstruierte Fassung nahm fälschlich `"model not
found: …"` an — keiner der `MODEL_NOT_FOUND_MARKERS` traf darauf zu; erkannt
wird dieser Fall seit Spec 0072 stattdessen strukturell über `error.type`
(s. `src/error.rs::is_structured_model_not_found`).

Alle vier Dateien sind gemessen (Modellname `no-such-model-xyz`). Keine
enthält Zugangsdaten oder kontokorrelierbare Werte.

Ollama (0.40.2), ebenfalls gemessen: `GET /v1/models` liefert das
OpenAI-Format (`{"object":"list","data":[{"id":…}]}`), und ein beliebiger
Bearer-Wert wird ignoriert (HTTP 200 für Modellliste und Chat). Ollama wird
strukturell über `error.type == "not_found_error"` erkannt.
