# 0038-pending-confirm-action-backend-timeout

## Status
Akzeptiert

## Kontext

Spec 0046, Fund 4 beschreibt einen Bug: "Tab schließen = wartende Aktion
ablehnen" (Spec 0017, Abschnitt 5) hängt am Frontend-State. Ein Reload
(Dev-Hot-Reload oder ein Frontend-Neustart) setzt diesen State zurück,
bevor der Schließen-Handler die wartende Aktion ablehnen konnte — das
Backend wartet dann auf `oneshot::Receiver.await` (`ConfirmationRegistry`,
`crates/app-shell/src/confirmation.rs`) ewig auf eine Entscheidung, die nie
kommt.

Die Spec ließ die Umsetzung bewusst offen und schlug zwei Wege vor:

1. **Backend-seitiges Timeout** für wartende `Confirm`-Aktionen (analog
   zum MCP-Timeout aus Spec 0028) — läuft es ab, gilt die Aktion als
   abgelehnt.
2. **State-Rekonstruktion** beim Frontend-(Re)start über `list_sessions`
   (Spec 0017), sodass der Schließen-Handler wartende Aktionen weiterhin
   kennt.

Mit der Empfehlung, das Backend-Timeout als robuste Grundsicherung zu
wählen und die State-Rekonstruktion optional obendrauf zu setzen.

## Entscheidung

**Nur das Backend-Timeout wurde umgesetzt, keine State-Rekonstruktion.**

- **Warum Backend-Timeout statt (nur) State-Rekonstruktion:** Rekonstruktion
  über `list_sessions` behebt ausschließlich den einen konkreten Fall
  "Frontend startet neu und lädt seinen State frisch". Sie hilft nicht,
  wenn das Frontend nach einem Reload aus einem anderen Grund gar nicht
  wieder online kommt (Absturz, geschlossenes Notebook, Netzwerktrennung
  bei einer Remote-Desktop-Sitzung, …) — in all diesen Fällen bräuchte es
  ohnehin ein Timeout als letzte Absicherung. Das Backend-Timeout deckt
  jede dieser Ursachen gleich ab, unabhängig vom WARUM. Eine
  State-Rekonstruktion zusätzlich zu bauen hätte für denselben Schutz
  keinen Mehrwert gebracht, nur zusätzliche Komplexität (der Schließen-
  Handler müsste dann zwei Quellen der Wahrheit — frisch geladener State
  und weiterhin laufendes Backend-Timeout — konsistent halten).
- **Warum 1 Stunde, nicht das kürzere MCP-Timeout (Spec 0028, Sekunden-
  bis Minuten-Bereich):** Das MCP-Timeout betrifft einen *externen
  Tool-Aufrufer*, der auf eine synchrone Antwort wartet — ein technischer
  Client, der nach wenigen Sekunden vernünftigerweise aufgibt. Das hier
  betroffene Timeout wartet auf eine *menschliche* Entscheidung in einem
  Bestätigungsdialog — ein Mensch, der einen riskanten Kommandovorschlag
  sorgfältig prüfen will, soll dabei nicht unter Zeitdruck stehen. Das
  Timeout ist ein Sicherheitsnetz gegen "nie" (der eigentliche Bug), keine
  UX-Grenze gegen "langsam". Eine Stunde ist großzügig genug, um in der
  Praxis nie einen echten, noch aktiven Bestätigungsvorgang zu
  unterbrechen, aber endlich genug, um verwaiste `oneshot`-Sender nicht
  dauerhaft im Speicher zu halten.
- **Abgelaufenes Timeout ⇒ ausschließlich `Deny`, nie `AutoExec`/Approve**
  (Spec 0046, Abschnitt "Sicherheits-Invarianten", explizit gefordert) —
  konsistent mit der sonstigen Eskalations-only-Richtung des Projekts.
- **`RejectionReason::Timeout`, nicht `User`:** ein unabhängiger
  `spec-reviewer`-Review dieses Schritts stellte fest, dass die erste
  Fassung eine Zeitüberschreitung als `RejectionReason::User` in die
  Chat-Historie schrieb — die KI (und ein Mensch, der die Historie später
  liest) hätte damit fälschlich angenommen, ein Mensch habe aktiv
  abgelehnt. Ein eigener `RejectionReason::Timeout`-Wert (`crates/core/
  src/ai/types.rs`) macht die tatsächliche Ursache ehrlich sichtbar, ohne
  die fail-safe Richtung zu ändern.

## Konsequenzen

- Eine wartende `Confirm`-Aktion, die nach einem Frontend-Reload verwaist,
  wird nach spätestens einer Stunde automatisch abgelehnt, nicht
  weiterhin ewig offengehalten. `ConfirmationRegistry::cancel(key)` räumt
  den verwaisten `oneshot::Sender` dabei aktiv aus der Map, statt ihn dort
  vergessen zurückzulassen.
- Falls sich in der Praxis zeigt, dass eine Stunde zu kurz (legitime,
  sehr lange Prüfpausen) oder zu lang (Nutzer erwarten schnelleres
  Feedback bei einem offensichtlich verwaisten Dialog) ist, ist die
  Konstante (`PENDING_ACTION_CONFIRM_TIMEOUT`,
  `crates/app-shell/src/orchestration.rs`) der einzige Anpassungspunkt.
- **Bewusst NICHT behoben in diesem Schritt:** derselbe Fehlerklasse
  betrifft auch die Host-Key-Bestätigung (`crates/app-shell/src/
  commands.rs`, `pending_host_key_confirmations`, ebenfalls über
  `ConfirmationRegistry`, aber ohne Timeout) — ein von `spec-reviewer`
  gefundener, aber außerhalb des Wortlauts von Spec 0046 Fund 4 ("wartende
  Confirm-Aktion") liegender Fund, als eigener Nachzieher-Task erfasst statt
  hier mitgezogen. Wichtig für einen künftigen Fix dort:
  `ConfirmationRegistry::cancel()`s Doc-Kommentar warnt inzwischen explizit
  davor, dass ein `cancel()` auf einem `SessionId`-Schlüssel eine
  zwischenzeitlich unter demselben Schlüssel neu registrierte Wartung
  (ein `connect()`-Retry nach `trust()`) versehentlich entfernen könnte —
  dort reicht das simple Muster von diesem ADR nicht ohne weitere
  Absicherung.
