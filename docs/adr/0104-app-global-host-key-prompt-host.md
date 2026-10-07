# ADR 0104 — Die Host-Key-Abfrage gehört einer stets gemounteten Komponente in `App`

Status: akzeptiert
Betrifft: Spec 0007, Spec 0100, ADR 0091, Issue #12

## Problem

Der einzige Listener auf `host-key-verification-needed` lebte in
`ServerList`. `ServerList` ist nur gemountet, solange im `MainScreen` der
Tab „Verbinden" aktiv ist; „Verwalten" und „Filter-Regeln" rendern andere
Ansichten. Löst das Backend selbst einen Verbindungsaufbau aus (z. B. über
MCP, `ensure_session` → `connect_session`), während einer dieser Tabs offen
ist, hört niemand das Ereignis. Die Verbindung wartet bis
`PENDING_ACTION_CONFIRM_TIMEOUT` und wird abgelehnt — sicher, aber die
Abfrage wird verpasst.

## Entscheidung

1. **`HostKeyPromptHost`** (`components/HostKeyPromptHost.tsx`) wird von
   `App` neben `ToastHost` gerendert, außerhalb aller Tab- und
   Session-Zweige. Es besitzt das einzige Abonnement auf
   `host-key-verification-needed`, den Zustand der offenen Abfrage,
   `HostKeyDialog` und den Aufruf von `confirmHostKey`.
2. **`ServerList` abonniert das Ereignis nicht mehr** — es gibt genau einen
   Empfänger, also nie zwei Dialoge für dieselbe Abfrage.
3. **Eine Abfrage zur Zeit**, wie bisher: ein neueres Ereignis ersetzt das
   angezeigte; das ersetzte läuft im Backend in den Timeout und gilt als
   Ablehnung. Keine Warteschlange.
4. **Fehler von `confirmHostKey`** (auch eine späte Entscheidung, nachdem
   das Backend bereits per Timeout abgelehnt hat) erscheinen als
   Fehler-Toast (`toastBus`, Spec 0067 B2 — bleibt bis zum Schließen
   stehen), nicht mehr im Fehlerbanner von `ServerList`.
5. **Schließen bei Verbindungsende:** `hostKeyPromptBus.ts`
   (`clearHostKeyPrompt()` / `subscribeHostKeyPromptClear()`, Bus-Muster
   wie `toastBus.ts`). `ServerList.performConnect` ruft im `finally`
   `clearHostKeyPrompt()` und behält so das bisherige Verhalten.
6. Eine Abfrage zu einem vom Backend ausgelösten Verbindungsaufbau, die das
   Backend bereits per Timeout abgelehnt hat, bleibt sichtbar, bis der
   Nutzer entscheidet; die Entscheidung scheitert dann mit einem
   Fehler-Toast. Das ist fail-closed. Ein Backend-Ereignis „Abfrage beendet"
   gehört nicht zu dieser Entscheidung.
   **Überholt durch ADR 0108 (Issue #37):** das Backend meldet das Ende
   jeder Abfrage über `host-key-verification-ended`, und
   `HostKeyPromptHost` schließt die passende Abfrage von selbst.

Unverändert: `connect.rs`, `confirm_host_key`, `trust_host_key`, der
Timeout als Ablehnung (`HostKeyWait::TimedOut`) und der eigene Dialog der
Test-Verbindung in `ServerForm` (anderer Pfad, über das Testergebnis statt
über das Ereignis). Das Portal von `HostKeyDialog` nach `document.body`
(ADR 0091) bleibt, weil `ServerForm` den Dialog aus seinem eigenen,
ausblendbaren Teilbaum heraus zeigt.

## Abgewogene Alternativen

- **`ServerList` in allen `MainScreen`-Tabs gemountet lassen** (nur per CSS
  ausblenden): kleinster Diff, aber die Sicherheitsabfrage hinge weiter am
  Lebenszyklus einer Listenansicht, und `ServerList` bräuchte einen eigenen
  Auffrischungs-Mechanismus, um nach Änderungen in „Verwalten" keine
  veraltete Liste zu zeigen.
- **So lassen und dokumentieren:** widerspricht dem Ziel, dass die Abfrage
  in jedem Tab-Zustand sichtbar ist.

## Konsequenzen

- App-weite, vom Backend angestoßene Abfragen gehören an die `App`-Wurzel,
  nicht in eine Tab-Ansicht.
- Tests: `App.hostKeyPrompt.test.tsx` prüft die Abfrage mit aktivem
  „Verwalten"-, „Filter-Regeln"- und Session-Tab, genau einen Dialog im Tab
  „Verbinden", den Fehler-Toast und das Schließen bei Verbindungsende.
