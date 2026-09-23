# Fixtures: „Modell nicht gefunden“ (Spec 0069, Teil 0.2; Spec 0072, A4)

| Datei | Status | Datum |
|---|---|---|
| `anthropic.json` | **gemessen** — echter Aufruf gegen `POST /v1/messages` mit falschem Modellnamen | 2026-09-23 |
| `openai.json` | rekonstruiert, unbelegt | — |
| `ollama.json` | rekonstruiert, unbelegt | — |
| `openrouter.json` | rekonstruiert, unbelegt | — |

`anthropic.json` trägt die tatsächliche Antwort (BL-0200, Messung
2026-09-23): `{"type":"error","error":{"type":"not_found_error","message":
"model: …"}}`. Die zuvor rekonstruierte Fassung nahm fälschlich `"model not
found: …"` an — keiner der `MODEL_NOT_FOUND_MARKERS` traf darauf zu; erkannt
wird dieser Fall seit Spec 0072 stattdessen strukturell über `error.type`
(s. `src/error.rs::is_structured_model_not_found`).

Die übrigen drei Dateien sind weiterhin aus öffentlich dokumentiertem/
bekanntem API-Verhalten rekonstruiert, **nicht** durch einen echten Aufruf
verifiziert (Key bzw. laufende Ollama-Instanz fehlten beim Messlauf). BL-0200
bleibt deshalb offen, bis auch sie gemessen sind. Keine der Dateien enthält
echte Zugangsdaten.

Vor dem nächsten Release bitte einmal gegen echte Accounts abgleichen
(s. „Manuelle Testabläufe“ im Abschlussbericht: „Falscher Modellname bei
OpenAI, OpenRouter, Ollama“). Weicht die Formulierung ab, die
Marker-Konstante `MODEL_NOT_FOUND_MARKERS` in `src/error.rs` entsprechend
nachschärfen.
