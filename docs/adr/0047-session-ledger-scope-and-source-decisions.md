# 0047 — Session-Ledger (Spec 0057, Etappe 1): Scope- und Herkunfts-Entscheidungen

## Status

Angenommen

## Kontext

Spec 0057 §1 verlangt ein append-only, verschlüsseltes, redigiertes Ledger
aller sicherheitsrelevanten Vorgänge einer Sitzung. Die Spec-Abschnitte
§1/§5 lassen mehrere Umsetzungsdetails offen, die während der
Implementierung von Etappe 1 entschieden werden mussten.

## Entscheidungen

### 1. Kein neuer `LedgerEntry`-Typ in `core::session` (Spec 0037)

`crates/core/src/session.rs` enthält bereits einen `LedgerEntry`-Typ aus
Spec 0037 — explizit als unbenutztes Zielbild-Vokabular dokumentiert
("bewusst (noch) nirgends im Code verwendet") und mit anderer Semantik
(`OutputDigest`, ein Hash statt der vollen Ausgabe). Spec 0057 §1.1
verlangt dagegen ausdrücklich die volle, redigierte Ausgabe ("Alles rein
(Kommandos, Ergebnisse, ...)"), weil das Ledger später als Grundlage für
Report/Audit-Log dienen soll.

Statt den Namenskonflikt durch Wiederverwendung/Anpassung des
Spec-0037-Typs zu lösen, lebt der neue Typ (`LedgerEntryContent`) in einem
eigenen Modul (`crate::audit` — ein bereits vorhandener, bislang leerer
Stub genau für diesen Zweck) mit einem expliziten Doc-Kommentar-Verweis
auf die Abgrenzung. `core::session` bleibt unverändert und weiterhin
unbenutzt.

### 2. `LedgerSource` als eigenes Enum, nicht `ActionOrigin`/`SessionOrigin`

Weder `app-shell::dto::ActionOrigin` (`Internal`/`Mcp`, lebt in
`app-shell`, nicht `core`) noch `core::session::SessionOrigin`
(`Human`/`McpAgent`, beschreibt die *Sitzung*, nicht einen einzelnen
Eintrag) decken den dritten nötigen Wert ab: eine Freigabe-Entscheidung
per Klick im Bestätigungsdialog ist weder "die KI" noch "ein MCP-Agent".
`LedgerSource{User, Ai, McpAgent}` ist deshalb ein eigenes, drittes Enum
in `core::audit`.

### 3. Scope-Reduktion: nur `AiAction::SuggestCommand` in Etappe 1

Spec 0057 §1.1 listet vier Ereignistypen, ohne den Aktionstyp
einzuschränken. Diese Etappe schreibt Ledger-Einträge ausschließlich für
`SuggestCommand`-Vorschläge — `ReadRemoteFile`/`WriteRemoteFile`/
`ProposeNoteUpdate` erzeugen (noch) keine Einträge. Begründung: Etappe 1
soll laut Spec 0057 §8 "das Grundgerüst" liefern, das Ledger soll
zunächst risikoarm "nebenher" laufen; die drei ausgelassenen Aktionstypen
sind seltener und ihre Abdeckung kann verlustfrei nachgezogen werden,
ohne das Schema oder die zentrale `write_ledger_entry`-Schreibstelle zu
ändern.

### 4. `LedgerSource`-Zuordnung für `Decision`-Einträge

- `AutoExec`/automatischer `Deny` (Filter-Engine allein entscheidet, kein
  Bestätigungsdialog gezeigt): Quelle = Herkunft des ursprünglichen
  Vorschlags (`Ai` bzw. `McpAgent`, aus `ActionOrigin` abgeleitet über
  `ledger_source_for_origin`).
- Ein tatsächlicher Klick im Bestätigungsdialog (`Approve`/`Deny`/
  `EditThenApprove`): Quelle = `User` — ein Mensch hat aktiv entschieden.
- Der Timeout-Fallback (`PENDING_ACTION_CONFIRM_TIMEOUT` abgelaufen, kein
  Mensch hat je entschieden, s. Spec 0046 Fund 4/`RejectionReason::
  Timeout`): Quelle = Herkunft des ursprünglichen Vorschlags, NICHT
  `User` — dieselbe Begründung wie bei `RejectionReason::Timeout` selbst
  ("kein Mensch hat hier tatsächlich entschieden").

### 5. Ledger unabhängig vom `persist`-Flag der Chat-Historie

`push_history_scoped`s `persist: bool` schließt MCP-Herkunft bewusst aus
der wiederaufnehmbaren Chat-Historie aus (Spec 0034/0040). Das Ledger
verwendet dieses Flag NICHT — Spec 0057 §1.1 verlangt explizit auch
`mcp-agent` als erfassbare Quelle ("für spätere Audit-„wer"-
Unterscheidung"). `write_ledger_entry` hat ein eigenes, unabhängiges Gate
(`session.ledger_store`/`session.chat_session_id` beide `Some`) und
schreibt für MCP-Herkunft genauso wie für interne Chat-Herkunft.

### 6. Zentrale Redaction in `write_ledger_entry`, bewusste Doppel-Redaction

Statt jede Aufrufstelle einzeln redigieren zu lassen (Fehlerquelle: ein
vergessener Aufrufer lässt unredigierte Daten durch), redigiert
`write_ledger_entry` selbst zentral, unmittelbar vor dem Schreiben. Für
`CommandExecuted` bedeutet das eine kleine, bewusst akzeptierte
Ineffizienz: der Aufrufer in `execute_suggested_command` redigiert
`output` bereits einmal für `chat_messages`/Log, und `write_ledger_entry`
redigiert intern erneut. Der Tradeoff (etwas doppelte Arbeit gegen eine
strukturell erzwungene, leicht verifizierbare Invariante) wurde bewusst
zugunsten der Invariante entschieden.

### 7. `EvaluationTrace`/`evaluate_explained` zusätzlich für die Kommandoschleife

`FilterEngine::evaluate_explained` war laut eigenem Doc-Kommentar
"ausschließlich für die Testen-Funktion im UI gedacht". Diese Etappe
nutzt sie zusätzlich in `orchestration::evaluate_action`, um
`matched_rule`/`matched_rule_origin` für Ledger-`Decision`-Einträge
verfügbar zu haben (das reicht `evaluate()` allein nicht). Der Trait
`CommandEvaluator` (`app-shell::session`) hatte zuvor zwei Methoden
(`evaluate`/`evaluate_explained`) — nach der Umstellung aller Aufrufer auf
`evaluate_explained` wurde das nun ungenutzte `evaluate` aus dem Trait
entfernt statt es als totes Interface zu belassen.

## Konsequenzen

- Kein Schema-/Typ-Konflikt mit dem unbenutzten Spec-0037-Vokabular.
- Das Ledger erfasst MCP-Aktivität vollständig, obwohl `chat_messages` sie
  weiterhin ausschließt — beide Verhalten sind jetzt unabhängig
  voneinander nachvollziehbar und einzeln regressionsgetestet.
- Die Abdeckung von `ReadRemoteFile`/`WriteRemoteFile`/`ProposeNoteUpdate`
  bleibt offen für eine spätere Erweiterung (kein technischer Blocker,
  s. Punkt 3).
