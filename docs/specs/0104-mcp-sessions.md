# Spec 0104 — Eigene Sitzung und eigener Tab für MCP-Anfragen

Status: umgesetzt · Issue: #50
Zweck: Aktionen, die ein externer MCP-Client (z. B. ein Coding-Agent)
anfragt, laufen in einer eigenen Sitzung mit eigener SSH-Verbindung und
eigenem Tab — getrennt vom Tab, in dem der Nutzer arbeitet. Terminal, Chat
und Fokus des Nutzers bleiben von MCP-Aktivität unberührt.
Review-Priorität: ERHÖHT (MCP-Vertrauensgrenze, Ausführungspfad,
Sitzungsaufbau)

Bezüge: Spec 0017 (Multi-Tab), Spec 0028 (MCP, Abschnitt 9a), Spec 0034/0040
(Chat-Persistenz, MCP ohne `chat_sessions`-Zeile), Spec 0039 (Fencing),
Spec 0057 (Kompaktierung, MCP-Ausschluss aus der Summary), ADR 0109.

## 1. Ist-Stand (vor dieser Spec)

- `AppMcpBackend::ensure_session` (`crates/app-shell/src/mcp_backend.rs`)
  nahm eine bestehende, verbundene Sitzung des Servers, falls eine offen
  war — MCP-Aktionen landeten dann im Tab des Nutzers. Sonst baute es eine
  Sitzung auf und schickte `mcp-action-tab-requested`; das Frontend holte
  den Tab dabei nach vorne (`useSessionTabs.ts`).
- MCP-Ergebnisse gingen in den KI-Verlauf dieser Sitzung (markiert über
  `mcp_origin_flags`, nicht persistiert). Die Chat-KI des Nutzers sah damit
  MCP-Ausgaben.

## 2. MCP-Sitzung

- **Schlüssel:** (Server, MCP-Client). Der Client ist `clientInfo.name` aus
  dem MCP-Handshake, getrimmt. Clients ohne Namen teilen sich je Server eine
  Sitzung — die App kann sie nicht unterscheiden.
- **Länge des Client-Namens (Issue #68):** höchstens 64 Zeichen
  (`MCP_CLIENT_NAME_MAX_CHARS`). Ein längerer Name wird an einer
  Zeichengrenze gekürzt (nie mitten in einem Multi-Byte-Zeichen), danach
  wird Leerraum am Ende entfernt. Der gekürzte Name gilt für den Schlüssel
  und für jede Anzeige: Tab-Beschriftung, OS-Benachrichtigung,
  Bestätigungsdialog. Zwei Namen, die sich erst hinter Zeichen 64
  unterscheiden, sind derselbe Client.
- **Höchstzahl je Server (Issue #68):** höchstens 4 offene MCP-Sitzungen je
  Server (`MCP_MAX_SESSIONS_PER_SERVER`), fest, nicht einstellbar. Es zählt
  jede eingetragene Sitzung, auch eine abgerissene, deren Tab noch offen
  ist. Eine Anfrage, die eine weitere Sitzung bräuchte, bekommt einen
  Fehler (`ActionOutcome::Failed` mit `MCP_SESSION_LIMIT_MESSAGE`). Es wird
  weder eine Verbindung aufgebaut noch ein Tab angelegt. Prüfen und
  Eintragen geschehen atomar, auch über verschiedene Clients hinweg. Die
  Wiederverwendung einer verbundenen Sitzung desselben Schlüssels ist davon
  nicht betroffen. Schließen eines MCP-Tabs gibt einen Platz frei.
- **Eigene SSH-Verbindung** über denselben `connect_session`-Pfad wie ein
  Sidebar-Klick: gleiche gespeicherte Zugangsdaten, gleicher
  Host-Key-Ablauf (unbekannter/geänderter Schlüssel → Abfrage wie bisher),
  `persist_chat_session: false` (Spec 0040, Abschnitt 4). Dem MCP-Client wird
  nichts Neues offengelegt.
- **Zuordnung** in `app_logic::mcp_sessions::McpSessionRegistry`
  (`AppState.mcp.sessions`). Sie kennt nur MCP-Sitzungen. Eine
  Nutzer-Sitzung desselben Servers ist dort nie eingetragen und wird für MCP
  deshalb nie verwendet, egal ob sie verbunden ist.
- **Wiederverwendung:** Eine MCP-Anfrage nimmt die eingetragene Sitzung
  ihres Schlüssels, solange sie verbunden ist. Ist die Verbindung
  abgerissen oder die Sitzung weg, legt die nächste Anfrage eine neue an.
  Ein noch offener alter Tab bleibt bis zum Schließen als MCP-Tab
  gekennzeichnet.
- **Gleichzeitige Anfragen** desselben Schlüssels warten auf ein
  Anlege-Lock je Schlüssel, damit nicht zwei Verbindungen entstehen. Andere
  Schlüssel (anderer Server oder Client) warten nicht, auch nicht auf einen
  offenen Host-Key-Dialog. Ein Lock-Eintrag besteht nur, solange eine
  Anfrage das Lock hält oder darauf wartet (auch ein abgebrochenes Warten
  meldet sich ab). Danach wird er entfernt, damit die Tabelle nicht mit
  jedem je gesehenen Client-Namen wächst (Issue #68). Das Eintragen der
  Sitzung geschieht immer unter dem Lock, eine spätere Anfrage findet sie
  deshalb auch mit einem neuen Lock.
- Die Sitzung wird **vor** dem Verbindungsaufbau eingetragen. So
  kennzeichnet `list_sessions()` den Tab schon während eines
  Host-Key-Dialogs, und Nutzer-Eingaben sind von Anfang an gesperrt.
  Scheitert der Aufbau, wird sie wieder ausgetragen.

## 3. Tab und Fokus

- `mcp-action-tab-requested` trägt zusätzlich `clientName`.
  `SessionSummaryDto` trägt `mcp: { clientName } | null`, damit ein Reload
  MCP-Tabs wieder als solche anzeigt.
- **Beschriftung** "<Client> @ <Server>", ohne Namen
  "Externes Tool @ <Server>", plus MCP-Abzeichen am Tab und im Kopf der
  Ansicht.
- **Kein Fokus-Wechsel:** Der MCP-Tab erscheint im Hintergrund. Der aktive
  Tab des Nutzers bleibt aktiv, ebenso die Übersicht. Nach einem Reload
  wird ein MCP-Tab nie automatisch aktiv. Ein Sidebar-Klick auf den Server
  wechselt zum Nutzer-Tab bzw. öffnet einen neuen, nie zum MCP-Tab.
- **Wartende Bestätigung:** Der MCP-Tab zeigt dann ein pulsierendes,
  beschriftetes Abzeichen („Bestätigung nötig“) statt nur des Punkts der
  Nutzer-Tabs. Dazu kommt wie bisher die OS-Benachrichtigung aus Spec 0028,
  Abschnitt 9a. Der Bestätigungs-Timeout aus Spec 0028, Abschnitt 7 bleibt
  unverändert.

## 4. Inhalt des MCP-Tabs

- Die Aktionskarten des Clients mit Bestätigen/Ablehnen und ihre Ergebnisse
  (Ausgabe, Dateiinhalt, Notiz-Diff), wie im Chat-Panel.
- **Kein interaktives Terminal, kein Dateibrowser, keine Chat-Eingabe.**
  Eine nur-lesende Sicht auf das, was lief, genügt (Issue #50). An Stelle
  der Eingabezeile steht ein Hinweis, dass die Sitzung einem externen Tool
  gehört.
- Das Backend setzt das zusätzlich durch: `send_chat_message`,
  `continue_truncated_response` und `open_terminal` lehnen eine
  MCP-Sitzung ab (`McpSessionRegistry::ensure_user_session`). So kommt kein
  Nutzer-Kontext in die MCP-Sitzung.

## 5. Schließen

- Schließen des MCP-Tabs (`disconnect`) trägt die Sitzung aus und
  **lehnt eine wartende Bestätigung ab** (fail closed), bevor die
  Verbindung getrennt wird (`McpSessionRegistry::end_session`). Die Aktion
  wird nicht ausgeführt und endet nicht erst am Timeout.
- Der MCP-Client bekommt für diese Aktion eine eindeutige Fehlermeldung
  („MCP-Sitzung wurde in der App geschlossen …“) statt „vom Nutzer
  abgelehnt“. Ein Ergebnis oder ein Filter-`Deny`, das vorher schon
  feststand, bleibt unverändert.
- Die nächste Anfrage dieses Clients legt eine neue MCP-Sitzung samt Tab
  an.
- Wird der MCP-Tab geschlossen, während der Verbindungsaufbau noch läuft
  (z. B. bei offenem Host-Key-Dialog), trennt `ensure_session` die danach
  doch aufgebaute Verbindung sofort wieder. Die Anfrage scheitert dann mit
  derselben Meldung.
- Das Ablehnen einer wartenden Bestätigung gilt für **jede** geschlossene
  Sitzung, nicht nur für MCP-Sitzungen (Issue #66): `disconnect` ruft
  `session::reject_pending_confirmation_on_close` auf. Der Schritt trägt
  zuerst eine MCP-Sitzung aus (`McpSessionRegistry::end_session`) und
  lehnt danach die wartende Bestätigung der Sitzung ab
  (`Session::reject_pending_confirmation`), ohne nach Herkunft der
  Sitzung zu unterscheiden. Für Nutzer-Tabs bleibt die Rückfrage im
  Frontend; dessen Ablehnung ist nur noch redundant, das Backend lehnt
  auch ohne sie ab (z. B. nach einem Frontend-Reload, wenn die
  `actionId` fehlt). War die Bestätigung schon aufgelöst, ist der Schritt
  ein No-op.

## 6. Unveränderte Invarianten

- Jede MCP-Aktion läuft weiter über
  `orchestration::handle_mcp_action_proposed`: erzwungenes `Confirm`
  (`FILTER_MCP_ORIGIN_REQUIRES_CONFIRM`), Filter-Engine, Redaction und
  Fencing (Spec 0039, ADR 0034) der MCP-Sitzung selbst.
- Kein Weg zum erhöhten SFTP-Kanal (Spec 0067):
  `test_ai_and_mcp_file_actions_never_use_the_elevated_channel` bleibt
  grün.
- `mcp_origin_flags` bleibt: Der Persistenz-Ausschluss (Spec 0040,
  Abschnitt 4) und der Summary-Ausschluss (Spec 0057, Abschnitt 2.1) hängen
  weiter daran. In einer MCP-Sitzung sind ohnehin alle Einträge MCP-Einträge.
- Die Trennung der Kontexte ist eine Verschärfung: MCP-Ausgaben erreichen
  die Chat-KI des Nutzers nicht mehr, und der Nutzer-Chat erreicht die
  MCP-Sitzung nicht.

## 7. Testbarkeit

`crates/app-logic/src/mcp_sessions/tests.rs`:

- Ein verbundener Nutzer-Tab auf Server X wird für MCP nie verwendet.
- Zwei Clients auf demselben Server bekommen zwei Sitzungen. Derselbe
  Client nimmt seine verbundene Sitzung wieder.
- Eine MCP-Aktion läuft nur über Transport und Verlauf der MCP-Sitzung,
  weiterhin als `Confirm` mit MCP-Code trotz Allow-Regel. Der Verlauf der
  Nutzer-Sitzung bleibt unverändert, und der Nutzer-Verlauf kommt nicht in
  die MCP-Sitzung.
- Schließen lehnt die wartende Bestätigung ab, nichts wird ausgeführt, die
  nächste Anfrage bekommt eine neue Sitzung — auch über den
  sitzungsunabhängigen Schließ-Schritt, und die Sitzung ist ausgetragen,
  bevor die Aktion zurückkehrt.

`crates/app-logic/src/orchestration/action_exec/tests_pending_confirmation.rs`
(Issue #66): Schließen einer Nutzer-Sitzung lehnt ohne Frontend-Aufruf ab
und verbucht das wie eine Nutzer-Ablehnung; bereits aufgelöste oder fehlende
Bestätigungen und Sitzungen, die nicht im `SessionManager` stehen, sind
No-ops.
- Nutzer-Eingaben werden nur für MCP-Sitzungen abgelehnt.
- Das Anlege-Lock serialisiert nur denselben Schlüssel.
- Issue #68: Ein zu langer Name wird gekürzt, Namen mit gleichem Anfang bis
  zur Grenze ergeben denselben Schlüssel, Multi-Byte-Namen ohne Panic. Bei
  erreichter Höchstzahl: Fehler, kein Eintrag, kein Tab-Event; Schließen
  gibt einen Platz frei; die eigene verbundene Sitzung wird weiter
  verwendet; eine abgerissene, offene Sitzung zählt mit. Nach Öffnen und
  Austragen hält `creation_locks` keine Einträge, auch nicht nach
  abgebrochenem Warten.

`crates/app-shell/src/mcp_backend.rs`: Rückmeldung an den Client bei
geschlossener Sitzung. Frontend: `useSessionTabs.test.ts` (kein
Fokus-Wechsel, Hinweis auf wartende Bestätigung, Reload,
`findExistingSessionId`), `SessionTabBar.test.tsx`, `SessionView.test.tsx`.

## 8. Offene Punkte

- Keine.
