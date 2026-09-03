# Spec: MCP-Server-Integration

Status: Entwurf
Modul: neue Crate `crates/mcp-server` (Free-Tier), Erweiterung
`crates/app-tauri`, `frontend/` (Einstellungen)
Abhängigkeiten: Kernschleife (Spec 0007/0021), Filter-Engine (Spec 0002),
KI-Aktionen (Spec 0003/0006/0020), strukturiertes Logging (Spec 0016)

## 1. Ziel

Smart SSH exponiert seine eigenen Fähigkeiten (Kommando vorschlagen, Datei
lesen/schreiben, Notiz aktualisieren, Server auflisten) als **MCP-Server**,
den externe MCP-Clients — allen voran Claude Code, aber auch Claude Desktop
oder andere — ansprechen können. Der Sinn: Ein externer Agent kann so beim
Debuggen "über SSH nachschauen", ohne rohen, ungeprüften SSH-Zugriff zu
bekommen — jede vorgeschlagene Aktion läuft durch **exakt dieselbe**
Kontroll-Infrastruktur wie ein intern von der App-eigenen KI vorgeschlagenes
Kommando.

**Nicht verhandelbar**: Kein zweiter, paralleler Ausführungspfad. MCP-
Tool-Calls werden auf dieselben `AiAction`-Varianten (Spec 0003/0020)
abgebildet und laufen durch dieselbe Filter-Engine, denselben Redactor,
denselben Bestätigungsdialog-Mechanismus wie jeder andere KI-Vorschlag.

## 2. Free vs. Premium — Schnitt für diese Spec

- **Free (Teil dieser Spec und des Implementierungs-Prompts)**: ein
  MCP-Server, ein lokaler Client, ein Bearer-Token, gebunden an `127.0.0.1`.
- **Premium (nur architektonisch vorgemerkt, nicht Teil des
  Implementierungs-Prompts)**: mehrere gleichzeitige externe Clients mit
  eigenen, granular gescopten Tokens (z. B. "dieser Agent darf nur Staging,
  nur lesend"), teamweite zentrale Audit-Sicht über mehrere Agenten hinweg,
  zentral verwaltetes Gateway für eine Organisation. Wird als eigene,
  spätere Crate (`crates/mcp-server-team` o. ä.) angelegt, sobald benötigt —
  hier nur als Grund für die Architektur-Entscheidung in Abschnitt 3
  relevant.

## 3. Architektur-Entscheidung: Wiederverwendung statt Parallelstruktur

`crates/mcp-server` implementiert einen MCP-Server (empfohlene Bibliothek:
das offizielle Rust-SDK `rmcp` — prüfe zum Implementierungszeitpunkt den
aktuellen Stand, das MCP-Ökosystem entwickelt sich noch), der eingehende
Tool-Calls **nicht selbst ausführt**, sondern in `AiAction`-Werte übersetzt
und an dieselbe Orchestrierungs-Funktion übergibt, die auch der interne
Chat-Flow nutzt (`handle_action_proposed` aus Spec 0007/0021). Dadurch ist
ein Bypass der Filter-Engine strukturell ausgeschlossen, nicht nur durch
Disziplin beim Programmieren — derselbe Code-Pfad wird zweimal angesprungen
(einmal vom Chat, einmal vom MCP-Server), nicht zweimal implementiert.

MCP-ausgelöste Aktionen laufen **außerhalb** der Turn-Fortsetzungslogik aus
Spec 0021 — es gibt keinen Chatverlauf, in den ein Ergebnis automatisch
zurückfließen müsste; der externe Client (z. B. Claude Code) verwaltet seine
eigene Fortsetzungslogik. Nach Abschluss (genehmigt/abgelehnt/blockiert)
geht das Ergebnis als MCP-Tool-Antwort zurück an den externen Client, fertig.

## 4. Angebotene Tools

```
list_servers()                              → informativ, kein Filter-Engine-Gate
get_server_notes(server_id)                  → effective_notes(), informativ
propose_command(server_id, command)          → AiAction::SuggestCommand
read_remote_file(server_id, path)            → AiAction::ReadRemoteFile
write_remote_file(server_id, path, content)  → AiAction::WriteRemoteFile
propose_note_update(server_id, new_content)  → AiAction::ProposeNoteUpdate
```

Bewusst **nicht** angeboten: Datei löschen/umbenennen/Verzeichnis anlegen —
dieselbe Begründung wie in Spec 0020, Abschnitt 4.3 für die interne KI.

## 5. Strengere Behandlung als interne KI-Vorschläge

Jede der oben genannten aktionsauslösenden Tools (`propose_command`,
`read_remote_file`, `write_remote_file`, `propose_note_update`) landet
**immer** bei einer Bestätigung im UI — unabhängig davon, ob eine
bestehende Allow-Regel das Kommando eigentlich automatisch ausführen würde.
Begründung: Ein externes Tool ist eine neue Vertrauensgrenze, die eine
bewusst striktere Behandlung verdient, unabhängig von bereits für die
interne KI eingerichteten Regeln — dieselbe Denkweise wie bei
SFTP-Schreibzugriffen (Spec 0020, Abschnitt 4.2). Diese Einschränkung ist
für die Free-Version fest codiert, keine Einstellung.

`list_servers`/`get_server_notes` sind rein lesende, ungefährliche
Informationsabfragen ohne Serververbindung und bleiben ohne Bestätigung
nutzbar.

## 6. Sicherheitsmechanismen

- **Nur `127.0.0.1`**, niemals im Netzwerk erreichbar.
- **Bearer-Token**, beim erstmaligen Aktivieren generiert, in den
  Einstellungen einsehbar/neu generierbar. Jeder Tool-Call ohne oder mit
  falschem Token wird abgelehnt, kein Teil-Zugriff.
- **Standardmäßig deaktiviert** — eigener Schalter in den Einstellungen,
  keine automatische Aktivierung.
- **Server-Allow-Liste**: eine explizite Auswahl, welche verwalteten Server
  überhaupt über MCP ansprechbar sind — nicht automatisch alle. Ein Server,
  der nicht auf der Liste steht, ist für `propose_command`/
  `read_remote_file`/`write_remote_file`/`propose_note_update` unsichtbar
  (Fehler "unbekannter Server", nicht "Zugriff verweigert" — kein
  Informationsleck über die Existenz nicht freigegebener Server).
- **Ursprungs-Kennzeichnung im UI**: Ein Bestätigungsdialog, der durch einen
  MCP-Tool-Call ausgelöst wurde, zeigt deutlich sichtbar "Angefragt über:
  externes Tool (MCP)" statt wie gewohnt den Namen des internen
  KI-Providers — der Nutzer muss immer erkennen können, ob eine Anfrage aus
  dem eigenen Chat oder von einem externen Agenten kommt.
- **Logging**: Jeder MCP-Tool-Call wird über die bestehende Infrastruktur
  aus Spec 0016 protokolliert, mit `origin: "mcp"` markiert — auch in der
  Free-Version, da das ein reiner Transparenz-Gewinn ohne Zusatzkomplexität
  ist.

## 7. Umgang mit wartenden Bestätigungen (Timeout)

Ein MCP-Tool-Call, der auf eine `Confirm`-Aktion trifft, blockiert, bis der
Nutzer in der App entscheidet — das ist gewollt (Abschnitt 5). Damit ein
wartender externer Client nicht unbegrenzt hängt (manche MCP-Clients haben
eigene Timeouts, ein ewig hängender Tool-Call ist schlechte UX), gilt ein
konfigurierbares Timeout (Default 5 Minuten): läuft es ab, bevor der Nutzer
entschieden hat, liefert der Tool-Call eine Antwort wie "Zeitüberschreitung
beim Warten auf Bestätigung — die Anfrage steht weiterhin in der App zur
Entscheidung offen" zurück, statt unbegrenzt zu warten. Die eigentliche
Bestätigungsanfrage im UI bleibt davon unberührt bestehen und kann weiterhin
normal entschieden werden, nur der ursprüngliche MCP-Tool-Call bekommt eine
Antwort, damit der aufrufende Agent nicht hängen bleibt.

## 8. Transport

HTTP-basiert (Streamable-HTTP-Transport gemäß aktuellem MCP-Standard,
prüfen zum Implementierungszeitpunkt), nicht stdio — Begründung: Die App
läuft bereits als langlebiger Prozess mit offenen SSH-Verbindungen; ein
stdio-basierter, vom Client gestarteter Subprozess hätte keinen Zugriff auf
diesen laufenden Zustand. Port konfigurierbar (Default z. B. `47823`,
außerhalb üblicher Kollisionsbereiche), zusammen mit dem Token in der
Konfiguration, die der Nutzer in seinen externen Client (z. B. Claude Codes
MCP-Konfiguration) einträgt.

## 9. UI (Einstellungen)

- Schalter "MCP-Server aktivieren" (Default aus)
- Angezeigter Verbindungs-Endpunkt + Token (mit "Neu generieren"-Button —
  invalidiert das alte Token sofort)
- Mehrfachauswahl: welche Server auf der Allow-Liste stehen
- Kurzer Hinweistext mit Beispiel-Konfiguration für Claude Code

## 9a. UI-Ablauf bei einer eingehenden MCP-Anfrage

Wichtige Ergänzung, die über die reine Kennzeichnung aus Abschnitt 6 hinausgeht:
Ein MCP-Tool-Call kann eintreffen, während der Nutzer die App gar nicht im
Vordergrund hat (z. B. gerade im Editor arbeitet, während Claude Code im
Hintergrund debuggt). Eine rein passive Anzeige (wie der Hintergrund-Tab-
Indikator aus Spec 0017, Abschnitt 5) würde in diesem Fall leicht übersehen
werden — die Anfrage liefe dann unbemerkt in den Timeout aus Abschnitt 7,
ohne dass der Nutzer je die Chance zur Bestätigung hatte. Deshalb:

- **Zielserver ist immer eindeutig sichtbar**: Jede aktionsauslösende
  MCP-Anfrage (`propose_command`, `read_remote_file`, `write_remote_file`,
  `propose_note_update`) öffnet — falls noch nicht vorhanden — automatisch
  einen neuen Tab für den betroffenen Server (Spec 0017-Infrastruktur
  wiederverwendet), statt eine unsichtbare Hintergrundverbindung
  aufzubauen. Existiert bereits ein Tab für diesen Server, wird dieser
  verwendet. Der Bestätigungsdialog selbst zeigt den Servernamen wie jeder
  andere Bestätigungsdialog auch — kein Sonderfall nötig, das ergibt sich
  automatisch daraus, dass er an eine konkrete, servergebundene Session
  hängt.

  **Keine vorherige manuelle Verbindung erforderlich**: Der Verbindungsaufbau
  läuft über denselben `connect()`-Pfad wie beim manuellen Klick auf einen
  Server in der Sidebar (Spec 0005/0007) — eine MCP-Anfrage an einen noch
  nie verbundenen Server baut die Verbindung selbst auf. Handelt es sich um
  die allererste Verbindung zu diesem Server (unbekannter Host-Key): Die
  Verbindung pausiert exakt wie bei einem manuellen Verbindungsaufbau
  (Spec 0005, Abschnitt 6) und wartet auf die Host-Key-Bestätigung des
  Nutzers, **bevor** das eigentliche Kommando überhaupt zur Bestätigung
  angezeigt wird. Dieser Host-Key-Dialog muss über denselben
  Tab-öffnen-plus-Benachrichtigung-Mechanismus sichtbar gemacht werden wie
  die eigentliche Aktions-Bestätigung — sonst würde eine MCP-Anfrage an
  einen neuen Server scheitern, ohne dass der Nutzer je die Chance zur
  Bestätigung bekommt.
- **Native Betriebssystem-Benachrichtigung**: Zusätzlich zur reinen
  In-App-Anzeige löst eine wartende MCP-Bestätigung eine native OS-Toast-
  Benachrichtigung aus (Tauri-Notification-Plugin), die den Servernamen
  nennt (z. B. "Claude Code möchte ein Kommando auf 'web-01' ausführen").
  Klick auf die Benachrichtigung holt das App-Fenster in den Vordergrund
  und springt direkt zum betroffenen Tab/Dialog. Das ist bewusst
  aufdringlicher als die stille Hintergrund-Tab-Markierung aus Spec 0017 —
  eine externe Anfrage, die auf eine Entscheidung wartet, ist ein anderer
  Dringlichkeitsgrad als ein Ergebnis aus dem eigenen Chat, den der Nutzer
  sowieso gerade aktiv verfolgt.
- **Herkunfts-Anzeige mit Client-Name, falls verfügbar**: Das MCP-Protokoll
  übermittelt beim Verbindungsaufbau optional Client-Metadaten
  (`clientInfo.name`, z. B. "Claude Code"). Ist dieser Wert vorhanden, zeigt
  der Bestätigungsdialog ihn statt der generischen Formulierung ("Angefragt
  über: Claude Code" statt nur "Angefragt über: externes Tool (MCP)") —
  fällt zurück auf die generische Formulierung, falls der Client keinen
  Namen übermittelt.
- **Nur die vier aktionsauslösenden Tools** lösen Tab-Öffnung/
  Benachrichtigung/Dialog aus. `list_servers`/`get_server_notes` bleiben
  bewusst still (kein Risiko, keine Bestätigung nötig, siehe Abschnitt 5) —
  sonst würde jede reine Metadaten-Abfrage unnötig aufdringlich wirken.

## 10. Testbarkeit

- Unit-Tests: Tool-Call → `AiAction`-Mapping korrekt für alle fünf Aktionen;
  `propose_command`/`write_remote_file`/etc. landen **immer** bei `Confirm`,
  auch mit passender Allow-Regel (expliziter Regressionstest gegen genau
  dieses Verhalten); Server außerhalb der Allow-Liste liefert "unbekannter
  Server", nicht "Zugriff verweigert"; falsches/fehlendes Token wird
  abgelehnt.
- Integrationstest: MCP-Server lokal hochfahren, Tool-Call über einen
  echten oder minimalen MCP-Test-Client absetzen, Timeout-Verhalten aus
  Abschnitt 7 verifizieren.

## 11. Offene Punkte

- Eine spätere Lockerung ("dieser MCP-Client darf whitelisted
  Read-Only-Kommandos automatisch ausführen") ist denkbar, aber bewusst
  nicht Teil der Free-Version — die Free-Version bleibt maximal
  konservativ (Abschnitt 5).
- Genaues Format der Premium-Scoped-Tokens (Abschnitt 2) ist nicht Teil
  dieser Spec, nur die Tatsache, dass die Architektur (Trennung
  MCP-Server-Crate von der Orchestrierungs-Logik) das später ohne
  Kern-Umbau zulässt.
