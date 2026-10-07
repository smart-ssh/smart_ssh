# ADR 0108 — Das Backend meldet das Ende einer Host-Key-Abfrage

Status: akzeptiert
Betrifft: Spec 0007, Spec 0068 (Teil 5b), ADR 0104 (Punkt 6), Issue #37

## Problem

ADR 0104 ließ eine Abfrage zu einem vom Backend gestarteten
Verbindungsaufbau (z. B. über MCP) sichtbar, nachdem das Backend sie per
Timeout bereits abgelehnt hatte. Eine späte Entscheidung scheiterte dann in
`confirm_host_key` mit einem Fehler-Toast — fail-closed, aber verwirrend.
Das Frontend erfuhr nie, dass das Backend nicht mehr wartet.

## Entscheidung

1. **Neues Ereignis `host-key-verification-ended`**
   `{ sessionId, promptId, reason: decided | timed_out | abandoned }`.
   Gesendet wird es bei **jedem** Ausgang des Wartens in `connect()`
   (`finish_host_key_wait`, direkt nach `clear_pending_connection`), auch
   nach einer Nutzerentscheidung. Das hält die Regel einfach: jede
   Abfrage, die mit `-needed` beginnt, endet mit genau einem `-ended`.
2. **`promptId` = Generation der Registrierung** in
   `pending_host_key_confirmations` (`RegistrationGeneration`, Spec 0068
   Teil 5b). Ein `connect()`-Retry registriert unter derselben `SessionId`
   neu und bekommt eine neue Generation. `host-key-verification-needed`
   trägt `promptId` additiv.
3. **`HostKeyPromptHost` schließt nur bei Übereinstimmung** von `sessionId`
   und `promptId` mit der angezeigten Abfrage. Ein Ende-Ereignis einer
   anderen Session oder einer älteren Abfrage derselben Session schließt
   nie eine neuere Abfrage.
4. **Bei `timed_out`** erscheint ein Hinweis-Toast (neue Toast-Art `info`,
   neutral, bleibt bis zum Schließen stehen): Die Abfrage ist abgelaufen,
   die Verbindung wurde nicht aufgebaut, dem Schlüssel wird nicht vertraut.
   Kein Fehler-Toast, kein Aufruf von `confirmHostKey`. Bei `abandoned`
   und `decided` schließt die Abfrage still.

Unverändert: `confirm_host_key` (prüft `promptId` nicht), `trust_host_key`,
`host_key_store.trust` und Timeout/Abbruch als Ablehnung. Das Ereignis ist
rein informativ und kann keine Vertrauensentscheidung auslösen oder
beschleunigen. `ServerList` schließt die Abfrage weiterhin zusätzlich über
`hostKeyPromptBus`, wenn sein eigener `connect()` endet (ADR 0104, Punkt 5).

## Abgewogene Alternativen

- **Nur bei `timed_out`/`abandoned` senden:** spart ein Ereignis pro
  Entscheidung, macht die Regel aber bedingt; das Frontend profitiert nicht
  davon, `decided` auszulassen.
- **Nur `sessionId` als Kennung:** ein verspätetes Ende-Ereignis der alten
  Abfrage hätte nach einem Retry die neue Abfrage geschlossen.
- **Fehler-Toast bei Timeout beibehalten:** ein Timeout ist kein Fehler des
  Nutzers, sondern die erwartete, sichere Ablehnung.

## Konsequenzen

- ADR 0104, Punkt 6 ist damit überholt.
- `promptId` ist ein Zähler pro App-Lauf (u64, beginnt bei 0) und nur
  innerhalb eines Laufs eindeutig — für die Zuordnung im Frontend genügt das.
- Tests: `commands::connect::host_key_wait_tests` (Ende-Ereignis bei
  Timeout, Abbruch und Entscheidung) und `App.hostKeyPrompt.test.tsx`
  (Schließen nur bei passender Kennung, Hinweis statt Fehler bei Timeout).
