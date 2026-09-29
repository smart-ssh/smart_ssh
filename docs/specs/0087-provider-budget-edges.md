# Spec 0087 — Budget and request edges of the AI providers

Status: freigegeben · Backlog: BL-0262, BL-0264, BL-0265 · Gate: —
Zweck: Ein OpenAI-kompatibler Server mit kleinem Kontext liefert eine Antwort
statt HTTP 400; kein Retry unterschreitet eine explizite Nutzereinstellung;
eine leere System-Nachricht geht nicht mehr hinaus.
Review-Priorität: NORMAL

## 1. Ist-Stand (Stand `3875032`)

**Budget-Retry beim Abschneiden.** Beide Provider wiederholen eine
abgeschnittene Runde einmal mit verdoppeltem `max_tokens`
(`RawEvent::RetryWithHigherMaxTokens` im `unfold` von `send`).
- OpenAI-kompatibel: `min(doppelt, model_max_tokens).max(bisher)` — ein
  expliziter `max_tokens_override` über dem Modell-Maximum wird nie
  unterschritten; Test
  `test_empty_length_retry_never_sends_less_than_an_explicit_max_tokens_override`.
- Anthropic: nur `saturating_mul(2).min(model_max_tokens)` ohne das
  `.max(…)`. Override 32 000, Modell-Maximum 16 384 → der Retry sendet
  16 384, weniger als eingestellt (gelesen, nicht ausgeführt). **BL-0265.**

**HTTP-Fehler.** `error::map_http_status` bildet jeden 400 ohne
Modell-Merkmal auf `AiError::ProviderUnavailable("HTTP 400: …")` ab.
`AiError::ContextTooLarge` (Code `AI_CONTEXT_TOO_LARGE`, DE/EN-Text in
`common.json`) existiert, wird aber von keinem Provider erzeugt
(`grep -rn ContextTooLarge crates apps/*/frontend/src`: nur Definition,
`Display`, `code()`, Frontend-Liste, ein Doku-Kommentar in
`commands/ai_providers.rs`).

**Kontextgrenze bei OpenAI-kompatiblen Servern:**
- vLLM (Quellcode, gelesen): `max_tokens` > Kontext →
  HTTP 400 `max_tokens=8192 cannot be greater than max_model_len=4096. Please request fewer output tokens.`;
  Eingabe + Budget > Kontext → HTTP 400 `This model's maximum context length is 4096 tokens. However, you requested …`.
- OpenRouter (Fehlerbericht Dritter, nicht gemessen):
  `This endpoint's maximum context length is N tokens. However, you requested about M tokens (… of text input, … in the output). …`
- llama.cpp (**gemessen**, `llama.cpp:server` b11223, Kontext 512):
  `max_tokens: 8192` bei kurzem Prompt → **HTTP 200**, normale Antwort.
  Eingabe länger als Kontext → HTTP 400
  `{"error":{"code":400,"message":"request (931 tokens) exceeds the available context size (512 tokens), try increasing it","type":"exceed_context_size_error",…}}`.
  llama.cpp prüft also nur die Eingabe; ein kleineres Budget hilft dort nicht.

Der Default für unbekannte Modelle an fremden Endpunkten liegt bei 8192
(`openai_compatible_default_max_tokens`, Obergrenze
`OPENAI_COMPATIBLE_NON_OPENAI_ENDPOINT_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS`
16 384) und trifft damit jeden vLLM-Server mit höchstens 8192 Token
Kontext schon bei der ersten Anfrage (Eingabe + Budget > Kontext).

**Leerer System-Prompt.** `OpenAiCompatibleProvider::build_request_body`
setzt immer `{"role":"system","content": system_text}` an den Anfang, auch
wenn `system_text` leer ist. Leer ist er nur bei nativem Tool-Calling und
leerem `system_context`; der einzige Produktivaufrufer damit ist der
Zugangsdaten-Test (`classify_credential_test_result`, `system_context:
String::new()`). Anthropic lässt das Feld in diesem Fall seit Spec 0081 weg
(`test_no_system_field_when_system_text_is_empty_with_native_tool_calling`).
llama.cpp nimmt eine leere System-Nachricht an (gemessen, HTTP 200); vLLM,
OpenRouter, Z.ai nicht gemessen. **BL-0264 ist damit eine Angleichung, kein
belegter Fehler.**

Das Rate-Limit-Budget (`ProviderBudgetGuard`) wird im OpenAI-kompatiblen
Provider nie beschrieben (Doc-Kommentar am Feld `budget`); ein
zusätzlicher Request dort umgeht also keine Sperre.

## 2. Teil 0

Teil 0: entfällt — die Fehlertexte sind belegt (llama.cpp gemessen, vLLM aus
dem Quellcode, OpenRouter aus zitierten Berichten), der Rest ist Code im
Repo. Ist ein OpenRouter-Text anders formuliert, greift die Erkennung nicht
und es bleibt beim heutigen Verhalten (sichtbarer Fehler), kein
Zuschnittsproblem.

## 3. Ziel und Nicht-Ziele

Ziel: A1–A3 unten.

Nicht-Ziele:
- Kein Retry bei llama.cpps `exceed_context_size_error` und keine andere
  Einordnung dieses Fehlers (Eingabe zu lang; bleibt `ProviderUnavailable`).
- Keine Kontextgrenzen-Erkennung beim Anthropic-Provider (Modell-Maxima
  sind dort bekannt).
- Kein Rechnen mit den Zahlen aus dem Fehlertext; kein Ändern der Defaults
  oder Obergrenzen aus Spec 0080.
- Keine Änderung an Kompaktierung, Verlauf oder Einstellungsformular.

## 4. Anforderungen

**A1 — Kontextgrenze (OpenAI-kompatibel, BL-0262)**
- A1.1 MUSS: Eine HTTP-400-Antwort, deren Körper (ohne Groß-/Kleinschreibung)
  `maximum context length is` oder `cannot be greater than max_model_len`
  enthält, gilt als Kontextgrenzen-Fehler. Andere Statuscodes und andere
  400-Körper (insbesondere llama.cpps `exceeds the available context size`)
  bleiben wie heute. Ein 400, der als „Modell nicht gefunden" erkannt wird,
  bleibt `ModelNotFound`. Die Erkennung wirkt nur im OpenAI-kompatiblen
  Provider; Anthropic und die Modell-Ermittlung bilden Fehler weiter wie
  heute ab, auch wenn sie dieselbe Abbildungsfunktion nutzen.
- A1.2 MUSS: Bei einem Kontextgrenzen-Fehler wiederholt der Provider die
  Anfrage **einmal** mit `max_tokens` = 2048 (gleicher Feldname wie in der
  ersten Anfrage, `max_tokens` bzw. `max_completion_tokens`) — aber nur, wenn
  (a) das gesendete Budget **nicht** aus einem vom Nutzer gesetzten
  `max_tokens_override` stammt und (b) 2048 kleiner als das gesendete Budget
  ist und (c) in diesem Aufruf von `send` noch kein Abschneide-Retry lief
  (A1.4). Sonst kein Retry. Aus dem Override stammt das Budget genau dann,
  wenn kein `max_tokens_hint` gesetzt ist und der Override gesetzt ist; ein
  Hint eines Nebenaufrufs gilt nie als Nutzereinstellung.
- A1.3 MUSS: Kommt kein Retry zustande oder scheitert auch der Retry mit
  einem Kontextgrenzen-Fehler, endet die Runde mit `AiError::ContextTooLarge`
  — kein dritter Versuch, kein Hängen. Ausnahme: A1.4, zweiter Punkt.
- A1.4 MUSS: Je Aufruf von `send` läuft höchstens **einer** der beiden
  Retrys (Kontext-Retry nach A1.2 oder Abschneide-Retry
  `RetryWithHigherMaxTokens`), also höchstens zwei HTTP-Anfragen
  (429-Wiederholungen nicht mitgezählt):
  - nach einem Kontext-Retry endet eine Antwort, die sonst den
    Abschneide-Retry auslösen würde (abgeschnittener Tool-Call, leere
    `length`-Runde), direkt mit
    `AiError::ResponseTruncated`, ohne weitere Anfrage;
  - nach einem Abschneide-Retry endet ein Kontextgrenzen-Fehler direkt mit
    `AiError::ResponseTruncated` (die Antwort passt nicht ins Budget; der
    Hinweis von `ContextTooLarge` wäre hier der falsche Rat).
- A1.5 MUSS: Der Text zu `AI_CONTEXT_TOO_LARGE` (DE/EN im Frontend, DE im
  `Display` von `AiError`) nennt die Abhilfe: die Einstellung
  „Max. Antwortlänge (Tokens)" / „Max. response length (tokens)" (bestehendes
  Label `aiProvider.maxTokensOverrideLabel`) niedriger setzen oder einen
  neuen Chat beginnen.
- A1.6 MUSS: Der ursprüngliche 400-Körper wird wie jeder Providerfehler über
  den bestehenden redigierenden Fehler-Logpfad geloggt, bevor der Retry
  startet; ein erfolgreicher Retry erzeugt eine Debug-Zeile mit
  altem und neuem Budget (ohne Körper).

**A2 — Anthropic-Retry respektiert den Override (BL-0265)**
- A2.1 MUSS: Der Abschneide-Retry des Anthropic-Providers sendet nie weniger
  als das Budget der ersten Anfrage — gleiche Regel wie beim
  OpenAI-kompatiblen Provider (Ist-Stand §1).

**A3 — Keine leere System-Nachricht (OpenAI-kompatibel, BL-0264)**
- A3.1 MUSS: Ist der System-Text nach Anhängen des Fallback-Zusatzes leer
  oder besteht nur aus Leerraum (wie beim Anthropic-Provider), enthält `messages` keine System-Nachricht. Im Fallback-Modus (ohne natives
  Tool-Calling) ist er nie leer und bleibt unverändert.

## 5. Design

- Festes Retry-Budget 2048 statt Rechnen aus dem Fehlertext: drei
  Textformate mit Zahlen in Prosa wären fragil, Halbieren (8192 → 4096)
  scheitert bei genau 4k-Kontext weiterhin. Der Wert steht als benannte Konstante neben den
  übrigen Budget-Konstanten des Providers.
- `ContextTooLarge` bleibt eine Einheitsvariante; keine neue Variante, kein
  neuer Code, keine Änderung an `errorCodes.ts`. Der Zugangsdaten-Test ordnet
  sie wie bisher als `Unreachable` mit Code ein.

## 6. Sicherheits-Invarianten

- **Abgeschnittene Antworten** (Spec 0065): ein abgeschnittener Tool-Call
  wird nie ausgeführt oder vorgelegt. A1.4 ändert nur, ob vor
  `ResponseTruncated` noch ein Versuch läuft, nie, was mit dem
  abgeschnittenen Inhalt geschieht.
- **Redaction der Fehlerlogs** (Spec 0049): der neue Pfad loggt den 400 über
  dieselbe Funktion wie heute (API-Key und `extra_headers` redigiert); die
  Debug-Zeile aus A1.6 enthält nur Zahlen.
- **Nie hängen:** höchstens ein Kontext-Retry, Zeitgrenzen wie beim ersten
  Versuch.
- Filter, Risiko, Confirm/AutoExec, Credentials: nicht berührt.

## 7. Tests

Wiremock für OpenAI-kompatibel (bestehende Test-Crate); bisher gibt es dort
keinen 400-Fall. Jeder Test zählt die empfangenen Anfragen und liest die
gesendeten Körper.

- T1 (A1.2) vLLM-Text „`max_tokens=8192 cannot be greater than max_model_len=4096…`",
  400, danach 200 mit Antwort: Nutzer erhält die Antwort; zweite Anfrage
  trägt `max_tokens: 2048`, erste 8192. Scheitert heute (Fehler statt
  Antwort).
- T2 (A1.1) wie T1 mit „`This model's maximum context length is 4096 tokens. However, you requested …`";
  T2b mit OpenRouter-Text in Kleinschreibung. Beide: Retry mit 2048.
- T3 (A1.1, negativ) llama.cpp-Körper aus §1: genau **eine** Anfrage, Ergebnis
  `ProviderUnavailable`. Scheitert, wenn die Erkennung auf „context" allein
  oder den Status allein reagiert.
- T4 (A1.1, negativ) 400 mit Modell-nicht-gefunden-Körper, der zusätzlich
  „maximum context length is" enthält: `ModelNotFound`, eine Anfrage.
- T5 (A1.1, negativ) 413/500 mit Kontextgrenzen-Text: kein Retry, Ergebnis
  `ProviderUnavailable`.
- T6 (A1.2a) Override 8192 gesetzt, 400 mit vLLM-Text: genau eine Anfrage,
  `ContextTooLarge`. Scheitert, wenn der Retry den Override unterläuft.
- T7 (A1.2b) Nebenaufruf mit `max_tokens_hint` ≤ 2048, 400 mit vLLM-Text:
  eine Anfrage, `ContextTooLarge`.
- T7b (A1.2a) `max_tokens_hint` 4096 **und** Override 8192 gesetzt, 400 mit
  vLLM-Text, dann 200: Retry mit 2048, Antwort kommt an. Scheitert, wenn
  der Hint als Nutzereinstellung behandelt wird.
- T8 (A1.3) zweimal 400 mit Kontexttext: genau zwei Anfragen,
  `ContextTooLarge`, Stream endet.
- T9 (A1.4) 400 mit Kontexttext, dann 200 mit `finish_reason: length` ohne
  Inhalt: genau zwei Anfragen, `ResponseTruncated`; kein Körper mit
  `max_tokens` > 2048.
- T9b (A1.4) 200 mit `finish_reason: length` ohne Inhalt, dann 400 mit
  Kontexttext: genau zwei Anfragen, `ResponseTruncated`, keine Anfrage mit
  2048.
- T10 (A1.2) Reasoning-Modell (`max_completion_tokens`): Retry setzt dasselbe
  Feld, das andere Feld fehlt.
- T11 (A1.6) Log-Test: vor dem Retry **existiert** eine Fehler-Logzeile mit
  Status 400, der eingebettete API-Key erscheint darin nur redigiert; nach
  erfolgreichem Retry existiert eine Debug-Zeile, die 8192 und 2048
  enthält. Scheitert, wenn der Retry-Pfad das Loggen überspringt.
- T11b (A1.1, negativ) Anthropic-Provider, 400 mit „maximum context length
  is": genau eine Anfrage, `ProviderUnavailable`. Scheitert, wenn die
  Erkennung in der geteilten Abbildungsfunktion liegt.
- T12 (A1.5) Frontend: DE- und EN-Text von `AI_CONTEXT_TOO_LARGE` enthalten
  das jeweilige Label von `aiProvider.maxTokensOverrideLabel` (Test liest
  beide aus den Locale-Dateien, kein kopierter Text).
- T12b (A1.5) Rust: `AiError::ContextTooLarge.to_string()` enthält
  „Max. Antwortlänge".
- T13 (A2.1) Anthropic, Override über dem Modell-Maximum, abgeschnittene
  Runde: Retry-Körper trägt mindestens den Override. Scheitert heute
  (16 384 statt Override).
- T14 (A2.1) Anthropic ohne Override: Verdopplung bis zum Modell-Maximum
  wie bisher (bestehende Tests bleiben grün).
- T15 (A3.1) natives Tool-Calling, `system_context` `""` und `"  \n"`:
  `messages[0]` ist jeweils die Nutzernachricht, keine System-Nachricht.
- T16 (A3.1) Fallback-Modus, leerer `system_context`: System-Nachricht mit
  Fallback-Zusatz vorhanden.

## 8. Offene Punkte

Keine. Das feste Budget 2048 steht im Item selbst als Vorschlag; die
Abweichung für llama.cpp ist eine Tatsache (§1), keine Scope-Reduktion.

## 9. Klarstellungen

(leer)

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `fix(ai-providers): never send less than the first budget on the Anthropic truncation retry [BL-0265]` — A2, T13/T14.
2. `fix(ai-providers): omit an empty system message for OpenAI-compatible providers [BL-0264]` — A3, T15/T16.
3. `feat(ai-providers): retry once with a smaller budget when an OpenAI-compatible server reports its context limit [BL-0262]` — A1.1–A1.4, A1.6, T1–T11 inkl. T7b/T9b/T11b.
4. `fix(i18n): name the response length setting in the context-too-large message [BL-0262]` — A1.5, T12/T12b.
5. `docs(changelog): add fragment for the context limit retry [BL-0262]` — Fragment `changelog.d/0087-context-limit-retry.md`.

**Priorität:** NORMAL (kein Filter-, Risiko-, Credential- oder
Ausführungspfad; die Invariante zu abgeschnittenen Antworten prüft T9).

**Aufteilung:** ein Lauf, Sonnet. Alle Teile liegen in `ai-providers` bzw.
einer Locale-Zeile, ohne sicherheitskritischen Kern.

**Berührte Module:** `crates/ai-providers` (`openai_compatible.rs`,
`anthropic.rs`, ggf. `error.rs`, Tests), `crates/core/src/ai/types.rs`
(`Display`-Text), Frontend-Locales `de|en/common.json`, `changelog.d/`.

**Melde zurück:** welche Tests heute rot waren, wie A1.2a (Override-Herkunft)
bis in den Retry-Zustand gelangt, manueller Testablauf gegen einen
vLLM-Server mit `--max-model-len 4096`.
