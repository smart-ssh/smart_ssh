# Spec: Multi-Tab-Sessions

Status: Entwurf
Modul: `crates/app-tauri` (Session-Handling) + `frontend/` (Tab-Leiste,
Session-State)
Abhängigkeiten: Session-Handling und Kernschleife (Spec 0007), Titelleiste
(Spec 0014, dort bereits als Erweiterungspunkt vorgesehen), Prompt-Historie
(Spec 0015)

## 1. Ziel

Mehrere Server-Verbindungen gleichzeitig offen halten, zwischen ihnen per
Tab-Leiste wechseln — jede Session mit **eigenem** Terminal, eigenem
Chat-Verlauf und eigenem KI-Kontext. Bisher (Spec 0007) war die Annahme
"eine Verbindung pro geöffnetem Server, kein Pooling"; diese Spec hebt die
Beschränkung auf eine **gleichzeitig sichtbare** Session auf.

## 2. Backend: was sich ändert (und was nicht)

Erfreulich wenig: `AppState.sessions` ist bereits eine
`HashMap<SessionId, Session>` (Spec 0007, Abschnitt 3), und alle
bestehenden Commands (`send_chat_message`, `terminal_input`,
`respond_to_action`, …) nehmen bereits eine `session_id` entgegen. Mehrere
parallele Sessions sind damit **strukturell schon möglich** — bisher hat nur
das Frontend immer nur eine zur Zeit geöffnet.

Zu ergänzen:

```
list_sessions() -> Vec<SessionSummaryDto>
```

```rust
pub struct SessionSummaryDto {
    pub session_id: SessionId,
    pub server_id: ServerId,
    pub server_name: String,
    pub status: ConnectionStatus,   // Connected | Disconnected | AwaitingHostKey
    pub has_pending_action: bool,    // wartet auf eine Bestätigung durch den Nutzer
}
```

`list_sessions()` dient dem Wiederherstellen der Tab-Leiste, falls das
Frontend neu lädt (Dev-Modus/Hot-Reload) — das Backend ist die
maßgebliche Quelle dafür, welche Sessions tatsächlich offen sind, nicht der
Frontend-State.

Ebenfalls zu prüfen und ggf. zu korrigieren: **Nebenläufigkeit.** Mit
mehreren gleichzeitig aktiven Sessions laufen jetzt tatsächlich parallele
KI-Streams und Terminal-Reader-Tasks. Falls die bestehende
`SessionManager`-Implementierung einen einzelnen globalen Mutex über die
gesamte Session-Map hält, würde eine langsame KI-Antwort in Session A
Terminal-Eingaben in Session B blockieren — die Sperrgranularität muss so
gewählt sein, dass Sessions sich gegenseitig nicht ausbremsen (z. B. Mutex
pro Session statt einem über die ganze Map).

## 3. Frontend: Tab-Leiste

- Platzierung im `<AppHeader />` (Spec 0014, Abschnitt 6 hat das bereits als
  vorgesehene Erweiterung benannt), rechts neben dem App-Namen. Bei macOS
  weiterhin links Abstand für die Ampel-Buttons einhalten.
- Ein Tab pro offener Session: Servername, Statuspunkt (verbunden/getrennt),
  Schließen-Button. Klick auf einen Server in der Sidebar öffnet einen
  **neuen** Tab, statt den bestehenden zu ersetzen — außer für diesen Server
  ist bereits ein Tab offen, dann wird zu diesem gewechselt (kein zweiter
  Tab zum selben Server im MVP, siehe offene Punkte).
- **Tabs sind interaktive Elemente innerhalb der Drag-Region der
  Titelleiste** — sie müssen gemäß Spec 0014, Abschnitt 5 gezielt von der
  Drag-Region ausgenommen werden, sonst werden Tab-Klicks als
  Fenster-Ziehen interpretiert.
- Tastaturkürzel: `Cmd/Ctrl+W` schließt den aktiven Tab (mit Bestätigung,
  falls eine Aktion aussteht, siehe Abschnitt 5), `Cmd/Ctrl+Tab` bzw.
  `Cmd/Ctrl+1..9` wechseln zwischen Tabs.

## 4. Session-State im Frontend

Jede Session braucht ihren eigenen, isolierten State — der heutige
"eine Session"-State wird zu einer Map über `session_id`:

- Chat-Verlauf (inkl. laufender `chat-text-delta`-Streams)
- xterm.js-Instanz samt Scrollback
- aktuell wartender Bestätigungsdialog, falls vorhanden
- Prompt-Historie-Navigationszustand (Spec 0015) — der Entwurfs-Zwischen-
  speicher und Navigationsindex sind pro Tab getrennt, damit ein Tabwechsel
  keinen halbfertigen Entwurf verwirft

**Wichtig für die Event-Verarbeitung**: Alle Tauri-Events aus Spec 0007
(`terminal-output`, `chat-text-delta`, `chat-action-proposed`, …) tragen
bereits eine `session_id`. Das Frontend muss eingehende Events strikt der
zugehörigen Session zuordnen und darf sie **nicht** auf den gerade sichtbaren
Tab anwenden — sonst landet die Ausgabe einer Hintergrund-Session im
falschen Terminal. Das ist der wahrscheinlichste Fehlerfall bei dieser
Umstellung und verdient besondere Sorgfalt.

Hintergrund-Sessions laufen normal weiter: Terminal-Output wird weiter
empfangen und in den (unsichtbaren) xterm-Puffer geschrieben,
KI-Streams laufen weiter, ausstehende Bestätigungen bleiben erhalten.

## 5. Wartende Bestätigungen in Hintergrund-Tabs

Sicherheitsrelevantes Detail: Wenn eine Hintergrund-Session ein Kommando zur
Bestätigung vorlegt (`Confirm`, Spec 0007 Abschnitt 6), darf **kein**
Dialog im gerade sichtbaren Tab aufpoppen — das würde den Nutzer verleiten,
eine Bestätigung im falschen Kontext zu erteilen, ohne zu sehen, auf welchem
Server sie wirkt.

Stattdessen:
- Der betreffende Tab bekommt einen deutlich sichtbaren Hinweis-Indikator
  (`has_pending_action`), z. B. ein gelber Punkt.
- Der Dialog erscheint erst, wenn der Nutzer zu diesem Tab wechselt.
- Wird ein Tab mit ausstehender Bestätigung geschlossen, gilt das als
  **Ablehnung** der wartenden Aktion (nicht als Zustimmung) — plus eine
  Rückfrage vor dem Schließen, damit das nicht unbemerkt passiert.

## 6. Verbindungsabbau

`Cmd/Ctrl+W` bzw. der Schließen-Button ruft den bestehenden
`disconnect(session_id)`-Command auf. Damit greift automatisch auch der
KI-Notiz-Vorschlag beim Beenden (Spec 0010) pro geschlossenem Tab. Da dessen
Vorschlagsdialog laut Spec 0010, Abschnitt 2, Punkt 6 ohnehin schon
asynchron und tab-unabhängig als Benachrichtigung erscheinen soll, ist hier
keine Sonderbehandlung nötig — er darf nur nicht fälschlich an einen
inzwischen geschlossenen Tab gebunden sein.

## 7. Offene Punkte

- Mehrere Tabs zum **selben** Server (z. B. zwei parallele Shells auf einer
  Maschine) sind im MVP nicht vorgesehen — technisch möglich, aber wirft die
  Frage auf, ob sich beide Sessions denselben KI-Kontext/dieselbe
  Prompt-Historie teilen oder getrennt führen. Bewusst zurückgestellt, bis
  klar ist, ob der Bedarf besteht.
- Persistenz offener Tabs über einen App-Neustart hinweg ("Sitzung
  wiederherstellen") — nicht Teil dieser Spec, da ein Neustart ohnehin alle
  SSH-Verbindungen beendet und ein automatischer Reconnect zu mehreren
  Servern beim Start heikel wäre (Zugangsdaten, Host-Key-Dialoge). Falls
  gewünscht, eher als "zuletzt geöffnete Server"-Liste zum manuellen
  Wiederverbinden.
