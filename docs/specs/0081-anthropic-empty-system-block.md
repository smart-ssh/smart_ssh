# Spec 0081 — Anthropic: kein leerer System-Block mit `cache_control`

Status: **freigegeben** (Stefan, 2026-09-24) · Backlog: BL-0261 · Gate: —
Repo: **öffentlich** `smart-ssh` — `crates/ai-providers/src/anthropic.rs`
(Code und Tests)
Review-Priorität: **NORMAL**
Zweck: „Zugangsdaten testen“ mit einem Anthropic-Provider scheitert nicht
mehr an einem leeren System-Block.

## 1. Ausgangslage (gemessen)

- `classify_credential_test_result` (`crates/app-shell/src/commands.rs`)
  schickt als Probe einen `SessionContext` mit
  `system_context: String::new()` und der Nachricht „Hi“.
- `AnthropicProvider::build_request_body` übernimmt `system_context`
  unverändert. Nur im Fallback-Modus ohne natives Tool-Calling hängt es
  etwas an. Danach setzt es **immer**
  `system: [{"type":"text","text": system_text,"cache_control":{"type":"ephemeral"}}]`.
- Mit nativem Tool-Calling bleibt `system_text` leer. Die API antwortet
  mit HTTP 400 `system.0: cache_control cannot be set for empty text
  blocks`. Belegt im App-Log: `system_context:""`, `history:["Hi"]`,
  darauf der 400. Die Oberfläche meldet den Provider dann als nicht
  erreichbar, obwohl der Schlüssel gültig ist.
- Der reguläre Chat ist nicht betroffen: `build_session_system_context`
  beginnt immer mit einem festen, nicht leeren Einleitungstext.

## 2. Ziel und Nicht-Ziele

Ziel: Ist der fertige `system_text` leer, enthält der Request **kein**
`system`-Feld. Nicht-Ziele: Änderungen an der Probe in `commands.rs`, an
Tool-Breakpoints oder an der Caching-Strategie für nicht leere Prompts.

## 3. Anforderungen

**A1.** In `build_request_body`: Ist `system_text` nach dem optionalen
Fallback-Zusatz leer oder besteht nur aus Leerraum (`trim().is_empty()`),
wird das Feld `system` im Body weggelassen. T1 prüft zusätzlich `"  \n"`. Sonst
bleibt es genau wie heute, samt `cache_control`.

**A2.** Der Kommentar über `system_value`, `cache_control` werde
„unbedingt gesetzt“, nennt die Ausnahme für den leeren Text.

**A3.** Alle anderen Felder des Bodys bleiben unverändert (Modell,
`max_tokens`, `messages`, `tools` samt Breakpoint am letzten Tool).

## 4. Invarianten

- Ein nicht leerer System-Prompt trägt weiter genau einen
  `cache_control`-Breakpoint.
- Der Body enthält nie einen Textblock mit leerem `text` und
  `cache_control`.

## 5. Tests (`anthropic.rs`, neben den bestehenden Body-Tests)

- T1: natives Tool-Calling, `system_context: ""`: Der Body hat **kein**
  Feld `system`. Scheitert heute, denn der Body enthält den leeren Block.
- T2: Fallback-Modus, `system_context: ""`: Der Body hat `system` mit
  nicht leerem Text (der Fallback-Zusatz) und `cache_control`. Das ist
  ein Wächter: Der Fallback-Pfad bleibt gecacht.
- Die bestehenden Tests `test_system_block_carries_a_cache_control_breakpoint`,
  `test_cache_control_set_unconditionally_even_for_a_tiny_system_prompt`
  und `test_fallback_mode_has_no_tools_field_but_system_still_cached`
  bleiben unverändert grün.

## 6. Umsetzung

Ein Lauf, Sonnet. Gate laut `CLAUDE.md`, spec-reviewer NORMAL.
`CHANGELOG`: „Zugangsdaten testen“ für Anthropic schlug fälschlich fehl.
Manueller Test: Einstellungen → Anthropic-Provider → „Zugangsdaten
testen“ meldet Erfolg.

## 7. Offene Punkte

Keine.
