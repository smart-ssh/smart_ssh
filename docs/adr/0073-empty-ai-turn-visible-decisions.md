# ADR 0073: Entscheidungen bei der Umsetzung von Spec 0080 (leere KI-Runde)

Status: Angenommen
Bezug: docs/specs/0080-empty-ai-turn-visible.md, Commits `5a58622`,
`908605e`, `e4d2a5a` (Spec-Klarstellung), `74ac9c7` (A2) plus
Review-Nacharbeit (dieser Commit)

## Kontext

Ein `spec-reviewer`-Review (ERHÖHT, adversarial) der ersten Fassung fand
sechs Spec-Konformitäts-Punkte und einen sicherheitsrelevanten
Budget-Fund; keiner davon betraf den eigentlichen Tool-Call-Schutz oder
den Retry-Zähler (beide wurden im Review ausdrücklich als unverletzt
bestätigt). Diese ADR hält die dabei getroffenen Entscheidungen fest.

## 1. Die P1(b)-Klarstellung gilt NICHT für den Unbekannt-Fallback der offiziellen OpenAI-API

**Entscheidung**: `OPENAI_COMPATIBLE_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS`
(bisher eine gemeinsame Konstante für beide Fälle) ist jetzt zwei
getrennte Konstanten:
`OPENAI_COMPATIBLE_NON_OPENAI_ENDPOINT_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS`
(16384, wie in Spec 0080 §8 verlangt) und
`OPENAI_COMPATIBLE_OFFICIAL_API_UNKNOWN_MODEL_MAX_OUTPUT_TOKENS` (4096,
unverändert).

**Warum**: Die erste Fassung hatte beide Fälle weiterhin über dieselbe
Konstante verknüpft — ein unbekannter Modellname AN DER OFFIZIELLEN
OpenAI-API wäre damit stillschweigend von 4096 auf 16384 gesprungen, obwohl
Spec 0080 §8 die Anhebung wörtlich auf „Endpunkte außer der offiziellen
OpenAI-API" begrenzt. Gefunden durch das ERHÖHTE Review, per Test
abgesichert (`test_unknown_model_on_official_openai_falls_back_
conservatively_not_to_the_largest_value` erwartet jetzt explizit 4096).

## 2. Neue A4-Log-Zeile in die Diagnose-Allowlist

**Entscheidung**: `"AI response round ended (text/reasoning length)"`
(`ai_providers::request_logging::log_openai_round_summary`) steht jetzt in
`app_shell::diagnostics::SAFE_LOG_MESSAGES`.

**Warum**: Ohne diesen Eintrag hätte die fail-closed-Positivliste die neue
Zeile ausgeschlossen — und da sie die bisherige, bereits zugelassene
`"received text delta stream (summarized)"` für den OpenAI-kompatiblen
Provider ERSETZT (nicht ergänzt), wäre für diesen Provider im
Diagnose-Export genau die Information verschwunden, die Spec 0080 §1 als
Ausgangsproblem beschreibt ("sieht man im Log also nicht"). Kein
sensibler Inhalt: nur `request_id` + zwei Längenwerte, keine
Prompt-/Antworttext. Spec 0080 §8 stellt das ausdrücklich als Kann-Option
("falls sie dort erscheinen sollen") — hier bewusst mit Ja beantwortet, da
identisch zum bereits etablierten Präzedenzfall.

## 3. Retry-Deckel darf einen expliziten `max_tokens_override` nicht unterlaufen

**Entscheidung**: Beim A1-Retry wird der verdoppelte, am Modell-Maximum
gedeckelte Wert zusätzlich mit `.max(ursprünglicher max_tokens)`
verglichen — der Retry kann den tatsächlich verwendeten `max_tokens`-Wert
dadurch nie mehr UNTERSCHREITEN.

**Warum**: `max_tokens_override` (Spec 0065, Teil 4) hat Vorrang vor dem
modellabhängigen Default und wird bei der Anfangs-Anfrage bewusst nicht an
`model_max_tokens` gedeckelt. Ohne diesen Fix hätte ein Override, der über
dem Modell-Maximum liegt (z. B. 32000 bei einem Maximum von 16384 — genau
der in Spec 0080 §8/A3 empfohlene Ausweg für einen zu knappen Default),
den A1-Retry dazu gebracht, beim zweiten Versuch WENIGER Budget zu
schicken als der Nutzer explizit eingestellt hat (`min(verdoppelt,
kleineres Modell-Maximum)` kann unter den Ausgangswert fallen). Der Retry
wird für diesen Fall wirkungslos (derselbe Wert nochmal, dann beim zweiten
Fehlschlag ein sichtbarer Fehler) statt die Nutzereinstellung still zu
unterlaufen. Per Wiremock-Test mit Gegenbeweis abgesichert
(`test_empty_length_retry_never_sends_less_than_an_explicit_max_tokens_
override`).

**Bewusst nicht auf den Anthropic-Provider übertragen**: `crate::
anthropic::RetryState` hat denselben `saturating_mul(2).min(model_max_
tokens)`-Deckel (`anthropic.rs`, identisches Muster) und dieselbe
Override-Lücke. Spec 0080, Abschnitt 2 ("Nicht-Ziele"), schließt
Änderungen am Anthropic-Provider ausdrücklich aus — dieselbe Grenze wie
bei ADR 0072, Abschnitt "Bewusst nicht nachgezogen". Gehört als eigenes
Backlog-Item erfasst.

## 4. `chat-response-truncated`-Fallback: "letztes Element" statt "irgendein Assistenten-Element"

**Geprüft, nicht geändert**: A3, zweiter Punkt, spricht von "ohne
vorheriges Assistenten-Element". Implementiert ist "wenn das LETZTE
Element kein Assistenten-Element ist" (`ChatPanel.tsx`,
`onChatResponseTruncated`-Handler).

**Warum das bewusst so bleibt**: Innerhalb EINER Runde des OpenAI-
kompatiblen Providers ist die Kombination "Text + abgeschlossene Aktion +
`TextTruncated`" strukturell unerreichbar (`OpenAiStreamState::finalize`
gibt Tool-Calls nur frei, wenn `tool_call_truncated` — u. a. durch
`finish_reason: length` — `false` ist; ist es `true`, greift der
Retry-Pfad, nie `TextTruncated` mit bereits freigegebenen Calls). Über
mehrere Runden hinweg (Runde 1 mit Text + ausgeführter Aktion, Runde 2 —
eine automatische Folgerunde — leer und mit `length` abgeschnitten) wäre
die vom Review vorgeschlagene Alternative ("das letzte VORHERIGE
Assistenten-Element markieren") sogar FALSCH: sie würde eine bereits
vollständige, abgeschlossene Antwort aus Runde 1 nachträglich als
"abgeschnitten" kennzeichnen, obwohl nur Runde 2 (die gar keinen eigenen
Text hatte) betroffen ist. Die implementierte Fassung — ein neues, leeres
Element für die tatsächlich betroffene, inhaltsleere Runde — bildet den
Sachverhalt korrekt ab.

## 5. Zurückgestellte Funde (niedrigere Priorität, bewusst nicht in diesem Schritt behoben)

- **`content` als Array-Struktur** (manche Gateways liefern
  `content: [{"type":"text",...}]` statt eines reinen Strings) — der
  Parser (`.as_str()`) behandelt das wie leeren Content. Durch A1 neu
  erreichbar (führt zu einem Retry statt einer Textanzeige), aber
  sicherheitsmäßig unproblematisch (kein Freigeben ungeprüften Inhalts)
  und eine vorbestehende, von Spec 0080 unabhängige Parser-Lücke. Eigener
  Backlog-Kandidat.
- **A4 loggt nicht bei Transport-Fehler/Inaktivitäts-Timeout** (`finalize()`
  wird auf diesen Pfaden nicht aufgerufen). Vorbestehendes Muster
  (identisch zu `log_text_delta_summary` davor), der Fehlerfall selbst
  wird bereits über `log_provider_transport_error` sichtbar. Spec 0080
  formuliert "jede Runde", meint damit aber im Kontext von §1 erkennbar
  die Fälle, in denen die KI überhaupt geantwortet hat (kein Transport-
  Fehler) — keine Änderung.
- **„Weiter"-Instruktionstext an einer leeren Kürzungs-Karte**
  (`CONTINUE_TRUNCATED_RESPONSE_INSTRUCTION`, "Fahre exakt an der Stelle
  fort, an der sie endete") ist an einem leeren Assistenten-Element (A3,
  zweiter Punkt) inhaltlich unpassend, weil es keine Stelle gibt, an der
  fortgefahren werden könnte. Von A3 so verlangt (derselbe Knopf/Text für
  jede Kürzungs-Karte); eine Variante des Instruktionstexts für den
  Leer-Fall ist ein eigenständiger, kleiner UX-Backlog-Kandidat.
- **`text.trim().length > 0` blendet die Export-/Notiz-Leiste auch bei
  einer legitimen, aber reinen Leerraum-Antwort aus** — harmloser Randfall
  (eine Antwort aus nur Leerzeichen ist ohnehin nicht notizwürdig).
