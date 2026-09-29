# Spec 0092 — Rotes Risiko verlangt Bestätigung (Einstellung, Standard an)

Status: Vorschlag (Architekt) · Backlog: BL-0074 · Gate: —
Zweck: Ein rot eingestufter Vorschlag läuft nie ohne Rückfrage, solange die
neue App-Einstellung an ist — auch gegen eine Allow-Regel.
Review-Priorität: ERHÖHT

## 1. Ist-Stand (origin/main 42ac8b4, gelesen, nicht ausgeführt)

1. Die Risiko-Einstufung hat zwei Achsen, `server_risk` und `data_risk`,
   je `RiskLevel::{None, Yellow, Red}` (`core::risk::types::RiskAssessment`).
   Laut Doc-Kommentar ist sie „rein informativ — beeinflusst nie die
   `Decision`“. Das Frontend zeigt je Achse ein eigenes Badge
   (`ChatPanel.tsx`, `RISK_LEVEL_BADGE_CLASS`).
2. `handle_action_proposed` (`app-logic/src/orchestration/action_exec.rs`)
   berechnet nach `evaluate_action` die Einstufung
   (`risk_assessment_for_action`, regelbasiert) und eskaliert danach in
   einer Kette nur `AutoExec → Confirm`: Secret-Pfad, `sftp-server`,
   eingelesener Serverinhalt (`PostIngestPolicy`, nutzt `server_risk`),
   Injection-Verdacht (verbraucht das Flag per `swap`), MCP-Herkunft,
   gespeichertes Sudo-Passwort, frühere Ablehnung. Kein Glied prüft
   `Red`. Ein rot eingestuftes Kommando mit Allow-Regel läuft ohne diese
   Sonderfälle automatisch.
3. Secret-Pfad- und `sftp-server`-Glied stehen **vor** der
   Injection-Prüfung und lesen das Verdachts-Flag nur; ist es gesetzt,
   zeigen sie den Injection-Grund (`FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM`).
   Begründung im Kommentar dort: sonst verbraucht die eskalierte Aktion das
   Flag, und die eigentliche Folgeaktion liefe automatisch.
4. MCP läuft durch dieselbe Funktion (`handle_mcp_action_proposed` →
   `handle_action_proposed` mit `ActionOrigin::Mcp`) und wird ohnehin immer
   bestätigt.
5. Die Einstufung gibt es nur für Aktionen mit Pseudokommando
   (`pseudo_command_for_risk_classification`): `SuggestCommand`,
   `ReadRemoteFile`, `WriteRemoteFile`. `ProposeNoteUpdate` und
   `GenerateDocument` haben keine.
6. **Zweitmeinung:** Das Ereignis `chat-action-proposed` geht mit der
   regelbasierten Einstufung und der Entscheidung hinaus. **Danach** wird,
   falls konfiguriert, die KI-Zweitmeinung abgewartet
   (`fetch_second_opinion`, nur Daten-Achse, nur Anhebung:
   `escalate_data_risk`) und per `risk-assessment-updated` gemeldet. Erst
   dann läuft der `match prepared`: `AutoExec` führt aus. Eine Anhebung auf
   Rot ändert die Entscheidung heute nicht.
7. Der Empfänger für eine Bestätigung wird vor den `await`s registriert
   (`PreparedDecision::Confirm { pending }`, `PendingConfirmation::register`),
   damit ein früher Klick nicht verloren geht.
8. Die Zweitmeinungs-Einstellungen liegen app-weit in `settings.json`
   (`tauri-plugin-store`; Schlüssel `riskClassifierEnabled`,
   `riskClassifierProviderId`). Das Backend liest sie einmal in `connect()`
   (`app-shell::risk_second_opinion::resolve_second_opinion_provider`) und
   legt das Ergebnis auf die `Session`. Das Frontend liest und schreibt sie
   über `riskSettings.ts`; der Schalter liegt in `AiProviderSettings.tsx`.
9. Bestätigungsgründe werden über den `code` übersetzt: `errorCodes.ts` und
   `locales/{de,en}/common.json` (z. B. `FILTER_SFTP_SERVER_REQUIRES_CONFIRM`).
10. Andere Editionen binden diese Oberfläche und `app-shell` unverändert
    ein; sie brauchen keinen eigenen Teil.
11. Es gibt kein `THREAT-MODEL`-Dokument im Repo (`ls docs`); die
    Sicherheitsgrundsätze stehen im README, Abschnitt „Filter- & Policy-Engine“,
    und in ADRs (zuletzt `docs/adr/0082-…`).

## 2. Teil 0

Teil 0: entfällt. Alle tragenden Stellen sind gelesen. Welche Kommandos der
Klassifizierer rot einstuft, ermittelt der Coder für die Tests selbst über
den Klassifizierer (§7, Vorbemerkung). Das ist Testauswahl, kein Zuschnitt.

## 3. Ziel und Nicht-Ziele

Ziel: Neue app-weite Einstellung „Bei rotem Risiko immer nachfragen“,
Standard an. Ist sie an, wird jeder Vorschlag mit Rot auf **einer** der
beiden Achsen bestätigungspflichtig. Das gilt auch dann, wenn erst die
KI-Zweitmeinung auf Rot hebt.

Nicht-Ziele:
- Keine Änderung am Klassifizierer, an seinen Mustern oder an Gelb.
- Keine Abschwächung: `Deny` bleibt `Deny`, und keine bestehende
  Eskalation entfällt, auch nicht bei ausgeschalteter Einstellung.
- Keine Einstellung je Server, keine Datenbank-Migration.
- Kein Umbau der Zweitmeinung zu einem losgelösten Task.
- Keine editionsspezifischen Teile.

## 4. Anforderungen

**A1 — Einstellung**
- A1.1 MUSS: App-weiter boolescher Wert in `settings.json`, Schlüssel
  `redRiskAlwaysConfirm`.
- A1.2 MUSS: Fehlt der Schlüssel oder ist er kein boolescher Wert, gilt
  „an“ (fail-safe; betrifft auch bestehende Installationen).
- A1.3 MUSS: Das Backend liest den Wert bei `connect()` und hält ihn für die
  Dauer der Sitzung fest, wie die Zweitmeinung. Eine Änderung wirkt ab der
  nächsten Verbindung. Der Hinweistext am Schalter sagt das.
- A1.4 MUSS: Der Schalter steht im Einstellungsbereich des
  Risiko-Klassifizierers. Er ist unabhängig davon bedienbar, ob die
  Zweitmeinung an ist. Texte gibt es auf Deutsch und Englisch.

**A2 — Eskalation bei regelbasiertem Rot**
- A2.1 MUSS: Einstellung an, Entscheidung `AutoExec`, und `server_risk ==
  Red` **oder** `data_risk == Red` → `Confirm` mit Code
  `FILTER_RED_RISK_REQUIRES_CONFIRM` und einem Grund, der die rote Achse
  und deren Begründung nennt.
- A2.2 MUSS: Gilt für Chat und MCP gleichermaßen (ein Pfad).
- A2.3 MUSS: Das Glied liest das Injection-Flag nur und verbraucht es nicht.
  Ist es gesetzt, wird der Injection-Grund gezeigt, wie bei Secret-Pfad und
  `sftp-server` (Ist-Stand 3).
- A2.4 MUSS: Secret-Pfad- und `sftp-server`-Grund haben Vorrang: Greift
  eines davon, bleibt dessen Code.
- A2.5 MUSS: Einstellung aus → Entscheidung und Code exakt wie heute.

**A3 — Nachträgliches Rot durch die Zweitmeinung**
- A3.1 MUSS: Einstellung an, Entscheidung nach der Kette `AutoExec`, und die
  Zweitmeinung hebt `data_risk` auf `Red` → Die Aktion wird **nicht**
  automatisch ausgeführt, sondern wie ein `Confirm` mit
  `FILTER_RED_RISK_REQUIRES_CONFIRM` behandelt. Es gelten dieselbe
  Wartezeit, dieselbe Abbruchlogik, derselbe Hintergrund-Tab-Indikator und
  dieselben Ledger-Einträge (`Confirmed`/`Rejected` mit Code) wie bei
  jedem anderen `Confirm`.
- A3.2 MUSS: Das Frontend erfährt die geänderte Entscheidung über ein
  Ereignis (§5). Die Karte zeigt danach den Bestätigungsdialog mit dem
  übersetzten Grund. Ein Klick darauf erreicht die wartende Aktion. Die
  Registrierung liegt deshalb vor dem Ereignis.
- A3.3 MUSS: Ein Stopp (`auto_continue_stop`) hat Vorrang. Eine gestoppte
  Aktion wird wie heute übersprungen und bekommt keinen Dialog.
- A3.4 MUSS: Ablehnung setzt `earlier_rejection` wie jede andere Ablehnung.
- A3.5 MUSS: Hebt die Zweitmeinung nur auf Gelb, bleibt sie aus oder ist
  die Einstellung aus, ändert sich nichts gegenüber heute.
- A3.6 MUSS: Auch der Tab-Zustand im Frontend (Hinweis auf eine wartende
  Aktion, Ablehnen beim Schließen des Tabs) reagiert auf das Ereignis aus
  §5, nicht nur die Karte. Heute setzt ihn nur `chat-action-proposed` mit
  `Confirm` (`useSessionTabs.ts`).

**A4 — Dokumentation**
- A4.1 MUSS: Neuer ADR `docs/adr/0084-red-risk-requires-confirm.md`:
  Entscheidung, Standard, Verhalten bei der Zweitmeinung, Restfall (§6).
- A4.2 MUSS: Der README-Abschnitt „Filter- & Policy-Engine“ nennt die
  Einstellung, ihren Standard und dass sie auch eine Allow-Regel übersteuert.
- A4.3 MUSS: Ein Changelog-Fragment unter `changelog.d/`.
- A4.4 MUSS: Die Doc-Kommentare in `core::risk` („beeinflusst nie die
  `Decision`“) werden richtiggestellt und verweisen auf ADR 0084.

## 5. Design

- **Stelle in der Kette:** nach `sftp-server`, vor dem Glied für
  eingelesenen Serverinhalt. So bleibt A2.4 erfüllt, und das Flag wird
  nur gelesen (A2.3).
- **Ereignis für A3:** `action-decision-escalated` mit `sessionId`,
  `actionId`, `reason`, `code`. Ein neues Ereignis statt einer Erweiterung
  von `risk-assessment-updated`, weil das Badge-Update auch ohne
  Eskalation kommt. Die Karte behandelt es wie eine `Confirm`-Entscheidung
  aus `chat-action-proposed`. Das Ereignis wird erst nach
  `risk-assessment-updated` gesendet.
- **Anzeige des Grunds:** Der Dialog zeigt, wie bei allen bekannten Codes,
  den festen übersetzten Text zum Code. Die rote Achse samt Begründung
  steht in `reason` für Ledger und KI-Kontext; im Dialog sieht der Nutzer
  sie am Badge. Keine Interpolation nötig.
- **Hard-Blacklist:** liefert heute `Confirm` (`FILTER_HARD_BLACKLIST`),
  nicht `Deny`. Das Glied greift nur auf `AutoExec` und lässt den Code
  deshalb unberührt.
- **Sitzungswert:** ein `bool` auf der `Session`, gesetzt in `connect()`.
  In Test-Fixtures ist er standardmäßig `true`, sodass bestehende Tests
  mit roten Kommandos und Allow-Regel sichtbar brechen und angepasst
  werden müssen, statt still auf „aus“ zu laufen.
- Herleitung und verworfene Varianten: HQ-Beilage.

## 6. Sicherheits-Invarianten

- **Nur Eskalation:** Das neue Glied und A3 machen nur aus `AutoExec` ein
  `Confirm`. Kein Pfad erzeugt `AutoExec` oder hebt `Deny` auf.
- **Injection-Flag:** wird nicht zusätzlich verbraucht (A2.3). Das Muster
  bleibt dasselbe wie bei Secret-Pfad und `sftp-server`.
- **Bestätigung geht nicht verloren:** Registrierung vor dem Ereignis
  (A3.2); `PendingConfirmation` räumt wie bisher per `Drop` auf.
- **Fail-safe:** unlesbare oder fehlende Einstellung → an (A1.2).
- **Restfall (bewusst):** Nach Ausschalten gilt Rot wieder nur als Hinweis.
  Eine laufende Sitzung behält ihren Wert bis zur nächsten Verbindung. Der
  ADR nennt das.
- Keine neue Datensenke. Der Grundtext enthält die Musterbegründung des
  Klassifizierers, keine Kommandoausgabe.

## 7. Tests

Vorbemerkung: Für „rotes Kommando“ nimmt der Coder Beispiele, die der
regelbasierte Klassifizierer heute rot einstuft **und** die weder an der
Hard-Blacklist noch an Secret-Pfad oder `sftp-server` hängen. Er ermittelt
sie mit einem Test über den Klassifizierer, nicht per Hand, und nennt sie
im Bericht. Alle Backend-Fälle laufen mit einer Allow-Regel, die das
Kommando sonst automatisch ausführen ließe.

| # | Fall | Erwartet | Scheitert, wenn … |
|---|---|---|---|
| T1 | Server-Rot, Einstellung an | `Confirm`, `FILTER_RED_RISK_REQUIRES_CONFIRM`, keine Ausführung vor Klick | das Glied fehlt |
| T2 | Daten-Rot (Server nicht rot), an | wie T1 | nur eine Achse geprüft wird |
| T3 | T1 und T2 mit Einstellung aus | `AutoExec` wie heute | die Einstellung ignoriert wird |
| T4 | nur Gelb, an | `AutoExec` | Gelb mit eskaliert |
| T5 | Rot mit Deny-Regel | `Deny` bleibt | das Glied `Deny` überschreibt |
| T5b | Rot und Hard-Blacklist (liefert heute `Confirm` mit `FILTER_HARD_BLACKLIST`) | Code bleibt `FILTER_HARD_BLACKLIST` | das Glied einen vorhandenen `Confirm` umschreibt |
| T6 | Rot + gesetztes Injection-Flag | Injection-Code; Flag danach **noch gesetzt**; nächste grüne Aktion mit Allow-Regel wird per Injection eskaliert | das Flag verbraucht wird |
| T7 | Secret-Pfad-Lesen, das zugleich rot ist | Secret-Code | A2.4 verletzt |
| T8 | MCP, rot | Code `FILTER_RED_RISK_REQUIRES_CONFIRM` (das Glied steht vor dem MCP-Glied) | der MCP-Pfad das Glied umgeht |
| T8b | `sudo <rotes Kommando>`, Allow-Regel auf die Form ohne `sudo`, kein gespeichertes Passwort | Code `FILTER_RED_RISK_REQUIRES_CONFIRM` | die Dual-Text-Allow-Prüfung das Glied umgeht; stuft der Klassifizierer die `sudo`-Form nicht rot ein, im Bericht melden |
| T9 | `ReadRemoteFile` auf rot eingestuften Pfad, der **kein** Secret-Pfad ist | Code `FILTER_RED_RISK_REQUIRES_CONFIRM` | Pseudokommandos nicht geprüft; findet der Klassifizierer keinen solchen Pfad, im Bericht melden statt den Test aufzuweichen |
| T10 | Zweitmeinung (Mock) hebt Gelb → Rot, an | keine Ausführung; `action-decision-escalated` nach `risk-assessment-updated`; Bestätigen → genau eine Ausführung; Ledger `Confirmed` mit Code | A3 fehlt oder doppelt ausführt |
| T11 | wie T10, Ablehnen | keine Ausführung, `earlier_rejection` gesetzt, Ledger `Rejected` | Ablehnung wirkungslos |
| T12 | wie T10, Einstellung aus | Ausführung wie heute, kein neues Ereignis | A3.5 verletzt |
| T13 | Zweitmeinung hebt nur auf Gelb, an | Ausführung, kein neues Ereignis | Schwelle falsch |
| T14 | wie T10, Stopp während der Zweitmeinung | übersprungen, kein Dialog | A3.3 verletzt |
| T15 | wie T10, Klick-Timeout | wie jede Bestätigung: nichts ausgeführt | eigener Wartepfad ohne Timeout |
| T16 | Einstellung: Schlüssel fehlt / `false` / `"false"` (String) | an / aus / an | kein Fail-safe |
| T17 | Messfall, kein MUSS: mehrzeiliges Skript mit einer roten Zeile, Allow-Regel | Stuft der Klassifizierer es rot ein, `Confirm` mit neuem Code; sonst Befund in den Bericht, keine Änderung am Klassifizierer | Klassifizierer rot, aber `AutoExec` |
| U1 | Frontend: Karte bekommt `action-decision-escalated` | zeigt Bestätigungsdialog mit übersetztem Text; Klick sendet `respond_to_action` | Ereignis nicht verdrahtet |
| U2 | Frontend: Hintergrund-Tab bekommt `action-decision-escalated` | Tab zeigt den Hinweis auf eine wartende Aktion; Tab schließen lehnt genau diese Aktion ab | Tab-Zustand hört nur auf `chat-action-proposed` |
| U3 | Frontend: Schalter — Wert fehlt / `false` / kein boolescher Wert (`0`, `"false"`) | an / aus / an, wie das Backend (A1.2) | Frontend und Backend zeigen Verschiedenes |
| U4 | Locale-Parität de/en inkl. neuem Code | grün | Schlüssel fehlt |

## 8. Offene Punkte

Keine. Die drei Designfragen aus BL-0074 sind entschieden: Rot auf einer der
beiden Achsen zählt, nachträgliches Rot durch die Zweitmeinung wird
bestätigt, die Einstellung gilt app-weit.

## 9. Klarstellungen

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `feat(app-logic,app-shell): require confirmation for red-risk proposals [BL-0074]` — A1.2/A1.3 (Backend), A2, Tests T1–T9 (inkl. T5b, T8b), T16, T17.
2. `feat(app-logic): escalate to confirmation when the second opinion raises risk to red [BL-0074]` — A3 Backend, T10–T15.
3. `feat(frontend): setting and dialog for red-risk confirmation [BL-0074]` — A1.4, A3.2/A3.6 Frontend, U1–U4.
4. `docs: record red-risk confirmation decision [BL-0074]` — A4 (inkl. A4.4, Doc-Kommentare in `core::risk`, nach dem ADR).

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Allow-Regel plus rotes Kommando: Gibt es einen Weg an dem Glied vorbei,
  etwa MCP, `ReadRemoteFile`, `sudo`-Variante (Dual-Text) oder ein
  Mehrzeilen-Skript?
- Timing bei A3: Führt der `AutoExec`-Zweig aus, bevor die Zweitmeinung
  ausgewertet ist? Geht ein Klick zwischen Ereignis und Registrierung
  verloren?
- Injection-Flag: Verbraucht das neue Glied es, sodass die Folgeaktion
  automatisch läuft?
- Einstellung: Lässt sie sich durch einen fehlerhaften Wert in
  `settings.json` still auf „aus“ bringen?
- Verwechseln sich die Codes (A2.4), sodass ein Secret-Pfad-Dialog seinen
  genaueren Grund verliert?

**Aufteilung:** zwei Läufe.
Lauf 1 auf Opus: Schritte 1–2, also Ausführungspfad und Sitzungsaufbau.
Lauf 2 auf Sonnet, danach: Schritte 3–4 (Oberfläche, Texte, ADR, README,
Changelog). Das Ereignis aus §5 ist die Schnittstelle zwischen beiden
Läufen. Lauf 2 ruft den spec-reviewer mit Priorität NORMAL.

**Berührte Module:** `core` (nur Doc-Kommentare in `risk`), `app-logic` (orchestration, session), `app-shell`
(connect, Einstellungen lesen), Frontend `riskSettings.ts`,
`AiProviderSettings.tsx`, `ChatPanel.tsx`, `useSessionTabs.ts`, `events.ts`, `errorCodes.ts`,
Locales, `docs/adr/`, `README.md`, `changelog.d/`.

**Melde zurück:** die gewählten roten Beispielkommandos (T1/T2/T9) samt
Klassifizierer-Ergebnis, das Ergebnis von T17, jede Entscheidung, die du
treffen musstest, und einen manuellen Testablauf für A3 mit
Zweitmeinungs-Provider.
