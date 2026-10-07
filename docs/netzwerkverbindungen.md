# Netzwerkverbindungen von Smart SSH

Smart SSH baut keine Netzwerkverbindung auf, die der Nutzer nicht ausgelöst
hat: keine Telemetrie, keine Update-Prüfung, keine Lizenzprüfung, kein
Nachladen von Schriften, Skripten oder Bildern aus dem Netz. Diese Seite
listet jede Verbindung, die die App aufbauen kann (Issue #14).

## Übersicht

| Verbindung | Ziel | Auslöser | Verlässt den Rechner |
|---|---|---|---|
| SSH-Sitzung | der Server aus dem Profil, bei Jump-Hosts zuerst jeder Hop der Kette | Nutzer verbindet sich mit einem Server | ja |
| SSH-Verbindungstest | der Server aus dem Profil (inkl. Jump-Hosts) | Klick auf „Verbindung testen" | ja |
| Dateibrowser (SFTP), auch mit erhöhten Rechten | derselbe Server, über die bestehende SSH-Verbindung | Nutzer öffnet bzw. bedient den Dateibrowser einer Sitzung | ja (keine neue Gegenstelle) |
| KI-Chat | Basis-URL des aktiven KI-Providers | Nutzer schickt eine Chat-Nachricht; Folgeaufrufe desselben Chat-Zugs (Werkzeugergebnisse, Fortsetzung) | je nach Provider-URL¹ |
| Verlaufs-Verdichtung | Basis-URL des Sitzungs-Providers | in einer laufenden Sitzung, wenn der Chat-Verlauf zu lang für das Kontextfenster wird | je nach Provider-URL¹ |
| Zweitmeinung zum Risiko | Basis-URL des eigens gewählten Zweitmeinungs-Providers | in einer laufenden Sitzung, für jede vorgeschlagene Aktion; nur wenn eine Zweitmeinung eingerichtet ist | je nach Provider-URL¹ |
| Injection-Prüfung | Basis-URL des Zweitmeinungs-Providers | in einer laufenden Sitzung, für gelesene Ausgaben/Dateien; nur wenn die Prüfung eingerichtet ist | je nach Provider-URL¹ |
| Sitzungstitel | Basis-URL des Sitzungs-Providers | beim Trennen einer Sitzung, die mindestens eine Nutzer-Nachricht und noch keinen Titel hat | je nach Provider-URL¹ |
| Notiz-Vorschlag | Basis-URL des Sitzungs-Providers | beim Trennen einer Sitzung, in der mindestens ein Befehl ausgeführt wurde | je nach Provider-URL¹ |
| Notiz kürzen | Basis-URL des KI-Providers | Nutzer stimmt dem Kürzen einer zu langen Notiz zu | je nach Provider-URL¹ |
| Modellsuche | Basis-URL aus dem Provider-Formular (`/models`) | Klick auf „Modelle laden" im Provider-Formular | je nach Provider-URL¹ |
| Schlüsselprüfung | Basis-URL aus dem Provider-Formular | Klick auf „Zugangsdaten testen" im Provider-Formular | je nach Provider-URL¹ |
| Attestierungs-Abruf | die vom Nutzer im Provider hinterlegte Attestierungs-URL | Klick auf „Attestierung abrufen" in den KI-Einstellungen | je nach URL |
| Ollama-Suche | `http://127.0.0.1:11434/v1/models` | einmal beim Öffnen der KI-Einstellungen, wenn noch kein Ollama-Provider existiert; erneut nur per Klick auf „Erneut suchen" (Spec 0069, E1) | **nein, nur lokal** |
| MCP-Server (eingehend) | lauscht auf `127.0.0.1:47823`, Pfad `/mcp` | Nutzer schaltet den MCP-Server in den Einstellungen ein; danach bei jedem Start, bis er ihn ausschaltet | **nein, nur lokal** |

¹ Die Basis-URL legt der Nutzer im Provider fest. Für Anthropic oder
OpenAI ist das ein Dienst im Internet, für einen lokalen OpenAI-kompatiblen
Dienst (z. B. Ollama auf `127.0.0.1`) bleibt die Verbindung auf dem
Rechner.

## Hinweise

- **Automatische KI-Aufrufe** (Verdichtung, Zweitmeinung, Injection-Prüfung,
  Sitzungstitel, Notiz-Vorschlag) gibt es nur innerhalb einer Sitzung, die
  der Nutzer gestartet hat, und nur an Provider, die er selbst eingerichtet
  hat. Ohne KI-Provider fällt keiner dieser Aufrufe an.
- **Externe MCP-Clients** können über den lokalen MCP-Server Befehle auf
  freigegebenen Servern anstoßen. Die SSH-Verbindung dazu ist dieselbe wie
  oben; der Nutzer hat den Server freigegeben und den Client selbst
  eingerichtet.
- **Der lokale Pseudo-Server** startet einen lokalen Prozess, keine
  Netzwerkverbindung.
- **Benachrichtigungen, Dateidialoge, „Im Ordner zeigen", „Öffnen mit"**
  laufen über das Betriebssystem, nicht über das Netz.
- **Drittlizenzen:** Der Dialog lädt die mitgelieferte Datei
  `third-party-notices.txt` aus dem App-Paket, nicht aus dem Netz.
- **Keine Plugins** für Updater, Telemetrie, Lizenzprüfung oder freie
  HTTP-Anfragen aus dem Webview.

## Wie das abgesichert ist

- **Webview:** Die Content-Security-Policy in
  `apps/smart-ssh-community/tauri.conf.json` erlaubt mit
  `connect-src 'self' ipc:` nur die App selbst und die Tauri-IPC. Jede
  Verbindung aus der Oberfläche läuft über einen Tauri-Befehl im Backend.
  Ein Test schlägt fehl, sobald sich `connect-src` ändert.
- **Backend:** Ausgehendes HTTP gibt es nur über `reqwest` in
  `crates/ai-providers`. Ein Test prüft den Abhängigkeitsgraph
  (`cargo tree`, ohne Dev-Abhängigkeiten, auf jeder CI-Plattform) und
  schlägt fehl, sobald eine andere Crate direkt oder über eine
  Drittanbieter-Crate von einem HTTP-Client abhängt (`reqwest`, `ureq`,
  `hyper`, Updater-/HTTP-Plugins …). Erlaubt sind nur `ai-providers` →
  `reqwest` und `axum` → `hyper`/`hyper-util` (axum ist der Server des
  lokalen MCP-Listeners).
- `crates/mcp-server` nutzt `reqwest` nur in seinen Tests.

Beide Tests stehen in `apps/smart-ssh-community/tests/network_boundary.rs`.
Wer eine neue Verbindung einführt, trägt sie zuerst hier ein und erweitert
dann die Erlaubnisliste im Test.

Ob die laufende App wirklich nur die erwarteten Verbindungen aufbaut
(z. B. eine Stunde im Leerlauf hinter einem Proxy), gehört zur Prüfung vor
einem Release, nicht zu diesen Tests.
