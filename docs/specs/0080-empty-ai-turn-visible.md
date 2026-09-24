# Spec 0080 — Leere KI-Runde: nie mehr still verschwinden

Status: **freigegeben** (Stefan, 2026-09-24) · Backlog: BL-0259 · Gate: —
Repo: **öffentlich** `smart-ssh` — OpenAI-kompatibler Provider,
Orchestrierung, Chat-Oberfläche
Review-Priorität: **ERHÖHT** (berührt den Retry-Pfad, der abgeschnittene
Tool-Calls zurückhält; adversariale Fälle in §5)
Zweck: Liefert das Modell in einer Runde keinen Text, sieht der Nutzer
einen Hinweis. Eine leere, wegen des Längenlimits abgeschnittene Runde
versucht die App einmal mit mehr Budget.

## 1. Ausgangslage (gemessen)

- App-Log und DB einer echten Sitzung (OpenAI-kompatibler Endpunkt, ein
  Modell mit Denkphase, kein `max_tokens_override`): Fünf Nutzer-Nachrichten
  blieben ohne Antwort. Vier Runden endeten mit `finish_reason: length`,
  eine mit `stop`. Zu keiner gibt es eine Assistenten-Zeile.
- `openai_compatible_default_max_tokens`: Für unbekannte Modelle an
  Endpunkten außer der offiziellen OpenAI-API gilt 4096 als Maximum, der
  Default ist die Hälfte, also **2048**. `max_tokens_override` hat Vorrang.
- `OpenAiStreamState::handle_chunk` liest von `delta` nur `content` und
  `tool_calls`. `reasoning_content`/`reasoning` wird ignoriert.
- `OpenAiStreamState::finalize` löst `RetryWithHigherMaxTokens` aus (einmal,
  Budget verdoppelt, gedeckelt am Modell-Maximum, zweiter Fehlschlag →
  `Error(ResponseTruncated)`). Das geschieht nur bei angesammelten
  Tool-Calls, im Fallback-Modus bei einer erkannten Aktion. Eine
  abgeschnittene Runde nur mit Text wird zu `TextTruncated`.
- `log_text_delta_summary` schreibt `text_len` nur, wenn er größer als 0
  ist. Den gesuchten Fall sieht man im Log also nicht.
- Orchestrierung: Ein leerer Textpuffer wird weder in Ledger noch in die
  Historie geschrieben. `Done` sendet kein Frontend-Event.
  `TextTruncated` sendet `chat-response-truncated`.
- `ChatPanel`: `onChatResponseTruncated` verwirft das Event, wenn das
  letzte Element keine Assistenten-Nachricht ist.

**Nicht gemessen:** dass die Denk-Tokens das Budget verbrauchen. Die Spec
hängt nicht davon ab, A4 macht es künftig im Log sichtbar.

## 2. Ziel und Nicht-Ziele

Ziel: A1–A4. Nicht-Ziele: Denkinhalte anzeigen oder speichern; den Hinweis
persistieren (er gilt nur für die laufende Ansicht); die Budget-Grenzen
ändern (§7); Änderungen am Anthropic-Provider; Verhalten bei
Verbindungsabbruch, fehlendem `finish_reason` oder `content_filter`, das
bleibt wie heute.

Das Akzeptanzkriterium des Items „Budget reicht für Reasoning-Modelle“
verkleinert sich bei P1(a) auf: einmal verdoppeln, Hinweis auf
`max_tokens_override`. Das Kriterium „Hinweis, Weiter“ für eine leere,
abgeschnittene Runde gilt beim OpenAI-kompatiblen Provider so: Nach A1
kommt entweder Text (Retry erfolgreich) oder beim zweiten Fehlschlag die
bestehende Fehlermeldung, ohne „Weiter“. „Weiter“ bleibt dem leeren
`TextTruncated` anderer Provider vorbehalten (A3).

## 3. Anforderungen

**A1 — Retry bei leerer, abgeschnittener Runde.** Endet eine Runde des
OpenAI-kompatiblen Providers mit `finish_reason: "length"`, ist bis dahin
**kein** Textinhalt angefallen (die bestehende Summe der Text-Deltas ist 0,
in beiden Modi) und gibt es weder Tool-Calls noch eine erkannte Aktion,
dann gilt die bestehende Retry-Logik: einmal, Budget verdoppelt,
gedeckelt, zweiter Fehlschlag `Error(ResponseTruncated)`. Nur in diesem
Fall. Alle anderen Endungen verhalten sich wie heute.

**A2 — Leere Runde melden (Backend).** Endet eine Runde mit `Done` (nicht bei `TextTruncated`), ohne
dass in **dieser** Runde Text angefallen oder eine Aktion vorgeschlagen
wurde, sendet das Backend ein neues Event `chat-response-empty`
(`{ sessionId }`). Nichts davon geht in Ledger oder Historie. Auf einen
Fehler (`Error`) folgt kein solches Event.

**A3 — Leere Runde zeigen (Frontend).**
- Auf `chat-response-empty` hängt der Chat ein Hinweis-Element an: „Das
  Modell hat keine Antwort geliefert. Bei Modellen mit Denkphase hilft ein
  höheres Ausgabe-Limit in den Provider-Einstellungen.“ Es hat **keinen**
  „Weiter“-Knopf (der Fortsetzungstext spricht von einer abgeschnittenen
  Antwort und passt nicht) und keine Leiste für Export oder Notiz. Der
  Nutzer schreibt einfach weiter.
- Auf `chat-response-truncated` ohne vorheriges Assistenten-Element wird
  ein leeres Assistenten-Element mit Kürzungs-Hinweis und „Weiter“
  angehängt, statt das Event zu verwerfen. Die Leiste für Export und
  Notiz erscheint bei leerem Text nicht.
- Text der nächsten Runde landet nie in einem Hinweis-Element aus dem
  ersten Punkt, sondern in einem neuen Assistenten-Element.
- Texte fest auf Deutsch, wie der bestehende Kürzungs-Hinweis.

**A4 — Messbar machen.** Am Ende jeder Runde des OpenAI-kompatiblen
Providers stehen im INFO-Log `text_len` (**auch bei 0**) und
`reasoning_len`, die Länge der Denk-Deltas (`reasoning_content` bzw.
`reasoning`). Denk-Deltas werden gezählt, aber nie als Text weitergegeben
und nie dem Aktions-Parser des Fallback-Modus zugeführt.

## 4. Invarianten

- Denkinhalte erreichen weder Frontend, Ledger, Historie, Aktions-Parser
  noch Log. Geloggt wird nur ihre Länge.
- Der Retry aus A1 wiederholt nie eine Runde, die schon Text geliefert hat.
- Weiterhin höchstens ein Retry je Anfrage.
- Abgeschnittene Tool-Calls werden weiterhin nie weitergegeben
  (bestehender Schutz, unverändert).

## 5. Tests

Provider:
- T1: nur `finish_reason: "length"`, kein Inhalt → Retry statt
  `TextTruncated`. Scheitert heute. Der bestehende Test, der für leeres
  `length` `TextTruncated` erwartet, wird angepasst.
- T2 (Wächter): `length` mit Text „abc“ → `TextTruncated`, kein Retry.
- T3: Server-Mock, zweimal leere Antwort mit `length` → zwei Requests,
  der zweite mit verdoppeltem Budget, am Ende `Error(ResponseTruncated)`,
  kein `TextTruncated`. Scheitert heute.
- T4: wie T1 im Fallback-Modus.
- Adversarial:
  - T5 (Wächter): leer, `content_filter` → kein Retry, Verhalten wie heute.
  - T6 (Wächter): Verbindungsende ohne `finish_reason`, leer → kein Retry.
  - T7 (Wächter): Inhalt nur aus Leerzeichen, `length` → kein Retry (es gab Text).
  - T8 (Wächter): `length`, kein Text, aber ein halber Tool-Call → bestehender
    Tool-Call-Pfad, Call wird nicht weitergegeben.
  - T9 (Wächter): Fallback-Modus, Denk-Delta enthält einen gültig aussehenden
    Aktionsblock, Inhalt leer → keine Aktion, kein Text.
  Die Wächter T5–T9 sind auch mit dem alten Code grün; sie fangen eine zu
  breite Umsetzung von A1/A4.
- T10: nur Denk-Deltas, dann `stop` → kein Text, `reasoning_len` > 0 und
  `text_len = 0` im Rundenabschluss (wie der Coder es testbar macht).

Orchestrierung:
- T11: Runde nur mit `Done` → `chat-response-empty` einmal, keine
  Ledger-Zeile. Scheitert heute.
- T12 (Wächter): Text + `Done` → kein `chat-response-empty`.
- T13: Aktion + `Done` ohne Text → kein `chat-response-empty`.
- T14: `Error` → kein `chat-response-empty`.

Frontend:
- T15: `chat-response-empty` → Hinweis sichtbar, kein „Weiter“, keine
  Export-/Notiz-Leiste; ein danach eintreffendes Text-Delta erscheint in
  einem neuen Element.
- T16: `chat-response-truncated` direkt nach der Nutzer-Nachricht →
  Kürzungs-Hinweis mit „Weiter“ sichtbar. Scheitert heute.

## 6. Umsetzung

Ein Lauf, Sonnet, spec-reviewer ERHÖHT. Reihenfolge: A1 und A4, dann A2,
dann A3. Gate laut `CLAUDE.md`. `CHANGELOG`: „KI-Antworten ohne Text
verschwinden nicht mehr still“. Manueller Test: Denkmodell,
`max_tokens_override` klein (z. B. 300), längere Frage → Antwort nach dem
Retry oder sichtbarer Hinweis, nie nichts.

## 7. Offene Punkte (K3, Stefan)

**P1 — Budget für unbekannte Modelle** (heute Maximum 4096, Default 2048).
- (a) So lassen. A1 verdoppelt einmal auf 4096, der Hinweis verweist auf
  `max_tokens_override`.
- (b) Maximum 16 384, Default 8192. Risiko: Endpunkte mit kleinerem Limit
  antworten mit HTTP 400.

Empfehlung **(a)**. Die Grenze ist bewusst vorsichtig, und der Override
löst den Einzelfall ohne Risiko für andere Endpunkte.

## 8. Klarstellungen

- **P1 entschieden (Stefan, 2026-09-24): Variante (b), entgegen der ursprünglichen Empfehlung (a).** Für unbekannte
  Modelle an Endpunkten außer der offiziellen OpenAI-API gilt künftig ein
  Maximum von **16 384** und ein Default von **8192**. Der Retry aus A1
  verdoppelt also auf 16 384. Grundlage ist eine Recherche der
  Anbieter-Doku: Harte Output-Obergrenzen liegen bei den gängigen
  Anbietern bei 8192 oder höher, viele kappen still. HTTP 400 droht vor
  allem bei selbst gehosteten Servern mit kleinem Kontext. Dort hilft
  `max_tokens_override`. Die Vorgabe in §2 („Budget-Grenzen ändern“ als
  Nicht-Ziel) und der Satz zur Verkleinerung des Item-Kriteriums gelten
  damit nicht mehr. Zusätzlicher Test **T17**: Ein unbekanntes Modell an
  einem Nicht-OpenAI-Endpunkt ohne Override schickt 8192 im Request.
  Scheitert heute, weil 2048 geschickt wird. Bestehende Tests, die 2048
  bzw. 4096 für unbekannte Modelle erwarten, werden angepasst.
  Der Hinweistext aus A3 bleibt.

- Neue Log-Einträge aus A4 kommen in die Allowlist des Diagnose-Exports,
  falls sie dort erscheinen sollen.

- **Q-BL-0259-01 entschieden (Stefan, 2026-09-24): Variante (b).** A2
  gilt nur für eine Runde, die eine Nutzer-Nachricht beantwortet: Runde 1
  eines Turns (neue Nachricht oder „Weiter“) sowie jede spätere Runde, in
  die eingereihte Nutzer-Nachrichten eingespeist wurden. Eine automatische
  Folgerunde nach einer ausgeführten oder geblockten Aktion, die ohne Text
  endet, bleibt still. Zusätzliche Tests: (1) Aktion in Runde 1 ausgeführt
  bzw. geblockt, Folgerunde endet nur mit `Done` → **kein**
  `chat-response-empty`; (2) eingereihte Nachricht in einer Runde > 1,
  die Runde endet nur mit `Done` → `chat-response-empty`.
