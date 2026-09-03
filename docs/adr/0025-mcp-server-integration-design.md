# 0025-mcp-server-integration-design

## Status
Vorgeschlagen

## Kontext

`docs/specs/0028-mcp-server-integration.md` gibt Ziel, Tool-Liste,
Sicherheitsmechanismen und einen groben UI-Ablauf vor, überlässt aber die
konkrete Umsetzung mehrerer nicht-trivialer Punkte der Implementierung. Der
Implementierungs-Prompt selbst erwähnt zwei davon bereits explizit als
mögliche Abweichungspunkte ("prüfe zum Implementierungszeitpunkt den
aktuellen Stand" für `rmcp`/den Transport). Diese ADR sammelt die
tatsächlich getroffenen Entscheidungen.

## Entscheidungen

**1. `crates/mcp-server` hängt bewusst nicht von `crates/app-tauri` ab —
Trait-Inversion statt direkter Wiederverwendung.** Spec-Abschnitt 3 fordert,
dass MCP-Tool-Calls "an dieselbe Orchestrierungs-Funktion" gehen wie der
interne Chat-Flow, und deutet damit einen direkten Aufruf von
`orchestration::handle_action_proposed` aus der `mcp-server`-Crate an. Das
ist mit Cargo strukturell nicht möglich: Sobald `app-tauri` seinerseits
`mcp-server` einbindet, um den Server zu starten und zu stoppen (Teil 2,
Ein-/Ausschalten in den Einstellungen), entstünde ein zyklischer
Abhängigkeitsgraph (`app-tauri → mcp-server → app-tauri`). Stattdessen
definiert `crates/mcp-server::backend` einen schmalen `McpBackend`-Trait
(nur `AiAction`/`ServerId`-Typen aus `ssh-manager-core`, keine
`app-tauri`-Typen); `crates/app-tauri::mcp_backend::AppMcpBackend`
implementiert ihn und ruft darin direkt `orchestration::
handle_mcp_action_proposed` (ein schmaler, öffentlicher Wrapper um das
weiterhin modul-private `handle_action_proposed`) auf. Es gibt dadurch
weiterhin **strukturell genau eine** Implementierung dieses Traits im
Produktivbetrieb — kein zweiter, das Downgrade aus Abschnitt 5 umgehender
Ausführungspfad —, nur die Abhängigkeitsrichtung ist gegenüber der
Spec-Skizze umgekehrt.

**2. Downgrade auf `Confirm` (Abschnitt 5) sitzt in `handle_action_proposed`
selbst, gesteuert durch einen neuen `origin: ActionOrigin`-Parameter.**
Erzwungen durch Entscheidung 1: Da die Filter-Engine-Auswertung und die
Downgrade-Logik ohnehin in `orchestration.rs` liegen (nicht in der
MCP-Crate erreichbar), ist das zugleich der einzige Ort, an dem sich die
Verschärfung *und* ihr Regressionstest gegen die echte Filter-Engine
(`test_mcp_origin_downgrades_autoexec_to_confirm_despite_allow_rule`,
`crates/app-tauri/src/orchestration.rs`) sinnvoll platzieren lassen — ein
Test gegen ein Mock-`McpBackend` in `crates/mcp-server` hätte nur bewiesen,
dass die Tool-Schicht die `AiAction` unverändert durchreicht, nicht dass die
Verschärfung selbst greift. `ActionOrigin` lebt in `crates/app-tauri/src/
dto.rs` (nicht in `orchestration.rs`), weil dieselbe Information auch für
die Ursprungs-Kennzeichnung im `chat-action-proposed`-Event (Abschnitt 6/9a)
gebraucht wird — ein DTO-naher Typ statt eines orchestrierungsinternen.

**3. Ergebnisableitung für die MCP-Antwort läuft über einen event-fangenden
`EventEmitter`-Wrapper (`CaptureEmitter`), nicht über eine Erweiterung des
`bool`-Rückgabewerts von `handle_action_proposed`.** `handle_action_proposed`
gibt nur zurück, ob eine Folgerunde nötig ist (Spec 0021) — nicht das
tatsächliche Ergebnis; eine Nutzer-Ablehnung im Bestätigungsdialog erzeugt
noch nicht einmal ein Event (nur einen Kontext-Eintrag). `AppMcpBackend::
propose_action` reicht deshalb einen `CaptureEmitter` (statt der echten
`AppHandle`) in `handle_mcp_action_proposed` hinein, der jedes Event
unverändert an die echte `AppHandle` weiterreicht (die UI reagiert
unverändert normal) und zusätzlich `chat-action-proposed`s `decision`,
`chat-action-result`s `result` sowie `chat-error`s `message` mitschneidet.
Nach Abschluss des Aufrufs least sich daraus deterministisch ableiten, ob
genehmigt (Ergebnis-Event kam), fehlgeschlagen (Fehler-Event kam), von der
Filter-Engine blockiert (`Deny`-Entscheidung, kein Event) oder vom Nutzer
abgelehnt (`Confirm`-Entscheidung, aber weder Ergebnis- noch Fehler-Event)
wurde — ohne den bestehenden Rückgabewert/die bestehenden Aufrufer
anzufassen. Eine eigene `CaptureEmitter`-Instanz pro MCP-Aufruf schließt
Verwechslungen mit gleichzeitiger, unabhängiger Chat-Aktivität auf
derselben Session aus (die läuft weiter direkt über die `AppHandle`).

**4. `list_servers()`/`get_server_notes()` sind auf die Allow-Liste
gefiltert, nicht "alle verwalteten Server".** Die Spec sagt für die vier
aktionsauslösenden Tools explizit "unbekannter Server, nicht Zugriff
verweigert — kein Informationsleck über die Existenz nicht freigegebener
Server", äußert sich aber nicht dazu, ob `list_servers()` selbst bereits
alle Server zeigen darf. Würde `list_servers()` ungefilterte Namen
zurückgeben, wäre der Anti-Leck-Zweck der anderen Tools bereits
unterlaufen — ein Client wüsste dann ohnehin, welche Server existieren,
nur eben nicht, dass er nicht "darf". `list_servers()`/`get_server_notes()`
filtern daher konsistent auf dieselbe Allow-Liste.

**5. Timeout-Umsetzung (Abschnitt 7): der Hintergrund-Aufruf läuft als
eigene `tokio::spawn`-Task weiter, das Tool-Call-Handling raced nur einen
Timer dagegen — kein `tokio::time::timeout(...)` direkt um den Aufruf.**
Ein `tokio::time::timeout` hätte beim Ablaufen die noch laufende Future
gedroppt und damit den im `ConfirmationRegistry`-Eintrag wartenden
`oneshot`-Receiver verwaist — ein späterer Klick auf "Genehmigen" im UI
hätte dann sichtbar nichts mehr bewirkt (der Fehler, den Abschnitt 7 explizit
ausschließt: "die eigentliche Bestätigungsanfrage im UI bleibt davon
unberührt bestehen"). `crates/mcp-server/src/tool_server.rs::
run_confirmable` spawnt stattdessen den `McpBackend::propose_action`-Aufruf
separat und `select!`t nur die Timer-Zweig-Antwort dagegen; bei Timeout läuft
der Hintergrund-Aufruf unbeeinflusst weiter (durch einen dedizierten Test
mit künstlicher Verzögerung verifiziert,
`test_confirm_timeout_returns_timeout_message_without_cancelling_backend_call`).

**6. Klick auf die native Benachrichtigung: kein eigener
Action-Handler/Deep-Link, verlässt sich auf OS-Standardverhalten.** Spec
9a verlangt "Klick auf die Benachrichtigung holt das App-Fenster in den
Vordergrund und springt direkt zum betroffenen Tab". `tauri-plugin-
notification`s einfache `.show()`-API (`NotificationBuilder`) bietet keinen
Klick-Callback zurück in die App — dafür bräuchte es registrierte
"Action-Types"/`on_action`-Handler (mobil) bzw. eine deutlich aufwendigere
Konstruktion. Stattdessen: Der Tab-Wechsel passiert bereits **beim Eintreffen
der Anfrage** (`mcp-action-tab-requested`-Event, sofort verarbeitet, nicht
erst bei Klick), die Benachrichtigung selbst ist rein informativ. Ein Klick
auf eine Benachrichtigung der eigenen App aktiviert das Fenster ohnehin über
das Standard-OS-Verhalten (macOS/Windows/Linux Notification Center), ohne
dass die App das explizit programmieren muss — der einzige Unterschied zur
Spec-Formulierung ist, dass der Tab-Sprung nicht *durch* den Klick ausgelöst
wird, sondern ihm bereits vorausgegangen ist.

**7. Kein Port-Konfigurationsfeld in den Einstellungen.** Spec Abschnitt 8
nennt den Port als "konfigurierbar (Default z. B. 47823)", Abschnitt 9s
UI-Liste nennt aber nur "Angezeigter Verbindungs-Endpunkt + Token", kein
Port-Eingabefeld. `McpServerConfig` selbst unterstützt einen beliebigen
`bind_addr` (Rust-seitig also tatsächlich konfigurierbar), die UI bietet
aber keinen Weg, ihn zu ändern — der Port bleibt fest `47823`. Spätere
Ergänzung eines Eingabefelds wäre eine reine UI-Erweiterung ohne
Backend-Änderung.

**8. Der MCP-Server startet automatisch beim App-Start, falls beim letzten
Beenden aktiviert.** Nicht explizit gefordert, aber konsistent mit dem
Zweck der Funktion: Ein einmal für einen externen Agenten eingerichteter
Zugriff soll nicht nach jedem Neustart der App manuell erneut angeschaltet
werden müssen. Scheitert der Autostart (z. B. Port belegt), wird das nur
geloggt — der App-Start selbst blockiert nicht.

## Konsequenzen

**Positiv:**
- Kein zweiter Ausführungspfad trotz der erzwungenen Trait-Grenze — die
  Downgrade-Verschärfung sitzt an genau einer Stelle und ist gegen die
  echte Filter-Engine getestet.
- Der Timeout-Mechanismus kann nie einen bereits im UI wartenden Dialog
  "unsichtbar" verwaisen lassen — verifiziert per Test.
- `crates/mcp-server` bleibt vollständig unit-testbar ohne Tauri-Runtime
  (Mock-`McpBackend`, reiner `tokio`/`axum`-Stack).

**Negativ / Trade-off:**
- **Bekannte Race-Bedingung im Host-Key-Dialog**: `ServerList.tsx`s
  `pendingHostKey`-State ist ein einzelner Wert, nicht nach `sessionId`
  indiziert. Löst eine MCP-Anfrage an einen bislang unverbundenen Server
  eine Host-Key-Bestätigung aus, während der Nutzer *gleichzeitig* manuell
  zu einem anderen, ebenfalls noch unbekannten Server verbindet, überschreibt
  das zweite `host-key-verification-needed`-Event das erste kommentarlos —
  der zuerst ausgelöste Dialog verschwindet, ohne dass der zugehörige
  `connect()`-Aufruf je eine Antwort bekäme. Vor Spec 0028 war das kein
  praktisches Problem (immer nur ein aktiver, nutzergetriebener
  Verbindungsaufbau gleichzeitig); die neue Auto-Connect-Fähigkeit über MCP
  macht gleichzeitige Verbindungsversuche jetzt real möglich. Nicht behoben
  — bräuchte eine größere, hier nicht spezifizierte Umstellung auf einen
  nach `sessionId` indizierten, ggf. gequeuten Host-Key-Dialog-Zustand.
- Der Klick-auf-Benachrichtigung-Sprung ist genaugenommen ein
  Sprung-vor-Klick — funktional äquivalent für den Nutzer (der Tab ist
  bereits sichtbar, sobald die native Benachrichtigung erscheint), aber
  nicht wörtlich das in der Spec beschriebene Klick-getriebene Verhalten.
- Ohne Port-Eingabefeld muss ein Nutzer mit einer echten Portkollision auf
  47823 den Code ändern statt eine Einstellung — für die Free-Version als
  hinnehmbar bewertet (der Port liegt bewusst außerhalb üblicher
  Kollisionsbereiche, s. Spec Abschnitt 8).
