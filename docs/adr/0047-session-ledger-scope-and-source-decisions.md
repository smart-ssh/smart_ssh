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
  ("kein Mensch hat hier tatsächlich entschieden"), plus `code: "TIMEOUT"`
  statt des ursprünglichen Eskalationsgrunds, damit dieser Fall im Ledger
  nicht nur über `source` von einer echten Nutzer-Ablehnung unterscheidbar
  ist.
- **Ausnahme innerhalb `EditThenApprove`**: blockiert die Filter-Engine
  den vom Nutzer BEARBEITETEN Text bei der Re-Evaluierung erneut (z. B.
  Hard-Blacklist), ist diese `Decision` NICHT `User`, sondern — wie der
  automatische Fall oben — die Herkunft des *ursprünglichen* Vorschlags:
  die Engine hat blockiert, nicht der Mensch (der Klick auf "Ausführen"
  im Bearbeiten-Dialog war ja gerade der Versuch, den Text laufen zu
  lassen). Der begleitende `CommandProposed`-Eintrag für den bearbeiteten
  Text bleibt trotzdem `User` (der Mensch hat ihn verfasst) — Vorschlag
  und Entscheidung dürfen hier bewusst unterschiedliche Quellen tragen,
  spec-reviewer-Fund (Review dieses Schritts): ursprünglich nicht als
  Ausnahme dokumentiert, jetzt hier ergänzt statt (wie zunächst denkbar)
  den Code stattdessen auf ein pauschales `User` zu ändern — das hätte
  eine automatische Blockade fälschlich als Nutzer-Ablehnung ausgewiesen.

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
Ineffizienz — präzisiert nach spec-reviewer-Rückfrage (Review dieses
Schritts): `execute_suggested_command` übergibt an `write_ledger_entry`
bewusst das noch UNREDIGIERTE `output` und redigiert seine eigene, für
`chat_messages`/Log bestimmte Kopie separat danach — es handelt sich also
nicht um eine zweite Redaction DESSELBEN bereits redigierten Werts,
sondern um zwei unabhängige Redaction-Durchläufe über dieselben
Rohdaten, mit demselben Endergebnis. Der Tradeoff (etwas doppelte Arbeit
gegen eine strukturell erzwungene, leicht verifizierbare Invariante)
wurde bewusst zugunsten der Invariante entschieden.

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

### 8. Nacharbeiten aus dem `spec-reviewer`-Review dieses Schritts

Der pflichtgemäße `spec-reviewer`-Durchlauf (CLAUDE.md, "Verbindlicher
Review-Workflow", ERHÖHT) fand keinen sicherheitsrelevanten Befund
(keine Filter-Umgehung, keine Klartext-Secrets, keine `AutoExec`-
Aufweichung), aber mehrere Vollständigkeits-/Genauigkeits-Lücken im
Audit-Protokoll. Behoben, in separatem Commit:

- **`CommandExecuted`-Eintrag auch im Fehlerfall.** Vorher blieb ein
  Transport-/Kanalfehler beim Ausführen ganz ohne Ledger-Eintrag — es sah
  so aus, als sei das Kommando nie ausgeführt worden, obwohl ein solcher
  Fehler nichts darüber aussagt, ob es den Server bereits erreicht hat.
  Jetzt schreibt auch der `Err`-Zweig einen `CommandExecuted`-Eintrag,
  mit `exit_code: None` ("Ergebnis unbekannt", nicht "erfolgreich") und
  der Fehlermeldung in `stderr` (läuft dadurch durch dieselbe zentrale
  Redaction wie echte Kommando-Ausgabe).
- **Eskalationsgrund im finalen `Decision`-Eintrag erhalten.** Die
  `Decision::Confirm { reason, code }`, die eine MCP-/Sudo-Passwort-/
  Injection-Verdacht-Eskalation oder den Default-Fallback der
  Filter-Engine trägt, lag zum Zeitpunkt des Bestätigungsdialogs längst
  vor, ging aber für den *finalen* Ledger-Eintrag (nach `Approve`/`Deny`)
  bisher verloren (`reason`/`code` liefen dort fest auf `None`). Jetzt
  wird sie durch `handle_user_decision` durchgereicht und im finalen
  Eintrag mitgeschrieben (außer beim Timeout-Fallback, s. Punkt 4 oben:
  dort `code: "TIMEOUT"` statt des ursprünglichen Eskalationsgrunds).
- **`LedgerSource` für `CommandExecuted` nicht mehr aus `persist`
  abgeleitet.** Vorher `persist ? Ai : McpAgent` — funktionierte nur
  zufällig (weil `persist` heute `!Mcp` bedeutet) und schrieb ein vom
  Menschen editiertes Kommando (`EditThenApprove`) fälschlich `Ai` zu.
  `execute_action`/`execute_suggested_command` bekommen die
  `LedgerSource` jetzt explizit vom Aufrufer durchgereicht (Herkunft des
  Vorschlags bei `AutoExec`, `decision_source`/`User` bei einer
  Bestätigung).
- **`UNIQUE(session_id, sequence)`** auf `ledger_entries` ergänzt (additiv,
  vor dem ersten Push, s. Migration 0011) — ohne die Constraint könnten
  zwei gleichzeitige Appends derselben Sitzung (Chat-Turn + MCP-Aktion,
  laut Spec 0040 möglich) dieselbe Sequenznummer bekommen; ein Verstoß
  schlägt jetzt als (nicht fataler, nur geloggter) Insert-Fehler fehl
  statt eine Kollision still zu verschlucken.
- Fehlende Testabdeckung ergänzt: Redaction für `CommandProposed`/
  `AiMessage` (vorher nur `CommandExecuted.output` getestet), die
  Timeout-Attribution samt `TIMEOUT`-Code, beide `EditThenApprove`-Zweige
  (automatisch erneut blockiert / angenommen), und ein expliziter
  Negativ-Test für die Scope-Reduktion aus Punkt 3
  (`ReadRemoteFile` erzeugt keine Einträge).

Bewusst NICHT behoben (Begründung im Review-Bericht selbst, an den
Nutzer weitergereicht): verlorene `sub_command_traces` bei verkettetem
Kommando (nur eine `matched_rule` pro Eintrag) — Etappe-1-Scope, keine
Sicherheitsrelevanz, wäre eine größere Typ-Erweiterung; die Frage, ob
Etappe 2's geplanter UI-Hinweis "vollständig im Ledger" bei bereits
Transport-seitig gekapptem Output zutrifft — vorab vor Etappe 2 zu
klären, betrifft nicht diesen Schritt.

## Konsequenzen

- Kein Schema-/Typ-Konflikt mit dem unbenutzten Spec-0037-Vokabular.
- Das Ledger erfasst MCP-Aktivität vollständig, obwohl `chat_messages` sie
  weiterhin ausschließt — beide Verhalten sind jetzt unabhängig
  voneinander nachvollziehbar und einzeln regressionsgetestet.
- Die Abdeckung von `ReadRemoteFile`/`WriteRemoteFile`/`ProposeNoteUpdate`
  bleibt offen für eine spätere Erweiterung (kein technischer Blocker,
  s. Punkt 3).
