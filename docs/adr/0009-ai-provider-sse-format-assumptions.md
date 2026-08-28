# 0009-ai-provider-sse-format-assumptions

## Status
Accepted

## Kontext

`docs/specs/0006-ai-provider-trait.md` legt fest, *dass* `AiProvider::send()`
Text inkrementell über SSE/HTTP-Streaming liefern muss (Abschnitt 4), aber
nicht *wie* die konkreten Provider-APIs ihre Chunks auf Byte-/JSON-Ebene
strukturieren. Beim Implementieren von `crates/ai-providers` mussten dafür
Annahmen getroffen werden, die nicht gegen die echten APIs verifiziert
wurden (keine Live-Zugangsdaten in dieser Umgebung) — nur gegen
`wiremock`-Mocks, die exakt diese Annahmen zurückspiegeln.

Zusätzlich verlangt Abschnitt 6 der Spec nur für zwei HTTP-Statuscodes ein
konkretes `AiError`-Mapping (401 → `AuthenticationFailed`, 429 →
`RateLimited`); für alle übrigen nicht-erfolgreichen Codes bleibt offen,
welche `AiError`-Variante zutrifft.

## Entscheidung

**SSE-Framing** (`crates/ai-providers/src/sse.rs`, providerunabhängig):
Frames sind `event:`/`data:`-Zeilenblöcke, getrennt durch eine Leerzeile,
gemäß der allgemeinen SSE-Spezifikation. Mehrere `data:`-Zeilen im selben
Block werden mit `\n` zusammengefügt.

**OpenAI-kompatible Chat-Completions-API**
(`crates/ai-providers/src/openai_compatible.rs`): unbenannte `data:
{...}`-Frames (kein `event:`-Feld); Text-Fragmente unter
`choices[0].delta.content`; Tool-Call-Fragmente unter
`choices[0].delta.tool_calls[].function.{name,arguments}`, nach `index`
gruppiert und über mehrere Chunks hinweg als Teilstrings akkumuliert;
Stream-Ende durch das Literal `data: [DONE]`.

**Anthropic-Messages-API** (`crates/ai-providers/src/anthropic.rs`):
benannte Events (`event: <typ>`); `content_block_start` mit
`content_block.type` (`"text"`/`"tool_use"`) eröffnet einen nach `index`
adressierten Block; `content_block_delta` liefert `delta.type ==
"text_delta"` (Feld `text`) bzw. `"input_json_delta"` (Feld
`partial_json`, akkumulierend); `content_block_stop` schließt den Block ab
und löst bei `tool_use`-Blöcken das Parsen der akkumulierten JSON-Argumente
aus; `message_stop` beendet den Stream. Ein zwingendes, von der Spec nicht
erwähntes `max_tokens`-Feld wird mit einem hartkodierten Default (4096)
befüllt.

**HTTP-Fehler-Mapping** (`crates/ai-providers/src/error.rs`, geteilt
zwischen beiden Providern): 401/403 → `AuthenticationFailed`, 429 →
`RateLimited` (wie in Spec Abschnitt 6 vorgegeben); alle übrigen
nicht-erfolgreichen Codes (4xx/5xx) → `ProviderUnavailable`, da sie
typischerweise ein serverseitiges bzw. vorübergehendes Problem anzeigen
statt eines für die App handlungsrelevanten Spezialfalls wie bei 401/429.
Transportfehler (Verbindungsaufbau, Timeout, TLS) → `NetworkError`.

Beide Provider-Module dokumentieren diese Annahmen zusätzlich als
Modul-Doc-Kommentar direkt am Ort der Implementierung.

## Konsequenzen

**Positiv:**
- Die Annahmen sind an genau einer Stelle pro Provider dokumentiert und
  decken sich mit den `wiremock`-Tests in
  `crates/ai-providers/tests/{openai_compatible,anthropic}.rs` — weicht die
  reale API ab, schlägt ein Test fehl, statt dass das Verhalten still
  falsch bleibt.
- Das SSE-Framing selbst (`sse.rs`) ist providerunabhängig und rein
  (keine I/O), dadurch unabhängig von den obigen Annahmen unit-testbar.
- Das Fehler-Mapping ist an einer Stelle geteilt, verhindert Divergenz
  zwischen den beiden Provider-Implementierungen.

**Negativ / Trade-off:**
- Ungetestet gegen die echten Endpunkte. Weicht die tatsächliche
  OpenAI- oder Anthropic-API-Antwortstruktur von den obigen Annahmen ab
  (z. B. andere Feldnamen, zusätzliche Wrapper-Objekte, ein künftiges
  API-Versions-Update), liefern beide Provider bis zu einem Fix stille
  Fehlinterpretation statt eines Absturzes — einzelne nicht-parsebare
  Chunks werden bewusst ignoriert statt den Stream abzubrechen (s.
  Kommentare in `openai_compatible.rs`/`anthropic.rs`), was Robustheit
  gegen unbekannte Zusatzfelder erkauft, aber einen echten Formatfehler
  eventuell erst spät sichtbar macht (leerer/unvollständiger Text statt
  eines Fehlers).
- `max_tokens` ist für `AnthropicProvider` aktuell nicht konfigurierbar —
  ein hartkodierter Default kann für lange Antworten zu früh abschneiden.
- 5xx-Codes landen pauschal auf `ProviderUnavailable`, auch dort, wo ein
  spezifischeres Mapping (z. B. 400 als eigener "ungültige Anfrage"-Fall)
  für die UI hilfreicher wäre — bewusst zurückgestellt, bis ein konkreter
  Bedarf dafür auftritt.
