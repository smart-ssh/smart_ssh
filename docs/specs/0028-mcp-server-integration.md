# Spec 0028 — Lokaler MCP-Server

Status: umgesetzt
Zweck: Smart SSH bietet seine Fähigkeiten (Server auflisten, Notizen lesen,
Kommando vorschlagen, Datei lesen und schreiben, Notiz-Änderung vorschlagen)
als lokalen MCP-Server an. Ein externer Agent (z. B. ein Coding-Agent) kann
damit auf einem Server nachsehen, ohne rohen SSH-Zugriff zu bekommen: Jede
Aktion läuft durch dieselben Kontrollen wie ein Vorschlag der eingebauten KI
und muss immer in der App bestätigt werden.
Bezüge: Spec 0002 (Filter-Engine), Spec 0005 (Verbindungsaufbau, Host-Key),
Spec 0016 (strukturiertes Logging), Spec 0017 (Tabs), Spec 0020 (KI-Aktionen
auf Dateien), Spec 0039 (Fencing nicht vertrauenswürdiger Inhalte), Spec 0067
(erhöhter Dateibrowser), Spec 0088 (Warten auf Bestätigung), Spec 0092
(rotes Risiko), Spec 0101 (Ablage des Tokens), Spec 0104 (eigene Sitzung und
eigener Tab je MCP-Client), ADR 0025, ADR 0103.

## 1. Überblick

Der externe Client spricht den lokalen MCP-Server der laufenden App an
(§8), sieht nur freigegebene Server (§6) und kann Aktionen nur vorschlagen;
ausgeführt wird erst nach Bestätigung in der App (§5), in einer eigenen
Sitzung je Client (§9a, Spec 0104).

## 2. Umfang

Ein lokaler MCP-Server mit einem Bearer-Token für alle Clients, gebunden an
`127.0.0.1`. Mehrere Clients gleichzeitig sind möglich und bekommen getrennte
Sitzungen (Spec 0104), teilen sich aber Token und Server-Allow-Liste.

## 3. Kein zweiter Ausführungspfad

Ein Tool-Aufruf über MCP wird zu derselben KI-Aktion wie ein Vorschlag der
eingebauten KI (Kommando, Datei lesen, Datei schreiben, Notiz-Änderung) und
durchläuft dieselbe Filter-Engine, dieselbe Redaction, dasselbe Fencing und
denselben Bestätigungsablauf. Der MCP-Server führt selbst nichts aus.

Eine MCP-Aktion erscheint als Aktionskarte mit Bestätigen und Ablehnen, wie
ein Vorschlag im Chat. Was der Nutzer dort entscheidet, gilt für den
externen Client: Freigabe führt aus und liefert das Ergebnis, Ablehnung
liefert „Abgelehnt: <Grund>", ein Filter-`Deny` liefert die Begründung der
Regel ohne Ausführung.

MCP-Aktionen gehören zu keinem Chat-Verlauf und lösen keine Folgerunde der
eingebauten KI aus. Das Ergebnis geht als Tool-Antwort an den externen
Client zurück; dessen Fortsetzung steuert der Client selbst.

## 4. Angebotene Tools

| Tool | Wirkung | Bestätigung |
|---|---|---|
| `list_servers` | Name und Kennung jedes Servers auf der Allow-Liste | nein |
| `get_server_notes(server_id)` | effektive Notizen des Servers (eigene und Gruppen-Notizen) | nein |
| `propose_command(server_id, command)` | Shell-Kommando auf dem Server | immer |
| `read_remote_file(server_id, path)` | Datei per SFTP lesen | immer |
| `write_remote_file(server_id, path, content)` | Datei per SFTP schreiben, mit Sicherung der alten Fassung | immer |
| `propose_note_update(server_id, new_content)` | Änderung der Server-Notiz vorschlagen | immer |

Bewusst **nicht** angeboten: Datei löschen, umbenennen, Verzeichnis anlegen
(wie für die eingebaute KI, Spec 0020). Dateiaktionen über MCP nutzen nie
den erhöhten SFTP-Kanal (Spec 0067).

Inhalte vom Server gehen an den externen Client genauso geschützt wie an die
eingebaute KI (Spec 0039): Notizen aus `get_server_notes`, die
Kommandoausgabe aus `propose_command` (stdout und, falls vorhanden, stderr)
und der Dateiinhalt aus `read_remote_file` sind zuerst redigiert und danach
als nicht vertrauenswürdiger Inhalt gefenct, mit dem Kommando bzw. Pfad als
Quelle. Eingeschleuste Tags im Inhalt sind escapt und können den Fence nicht
schließen. Kurze Statusangaben der App (Exit-Code, Hinweis auf einen Abbruch
durch den Nutzer) stehen außerhalb des Fence; die Rückmeldungen von
`write_remote_file` und `propose_note_update` enthalten keine Server-Inhalte
und sind nicht gefenct. Die Tool-Beschreibungen weisen den Client darauf
hin, dass gefencter Inhalt Daten ist, keine Anweisung.

## 5. Immer bestätigen

Jedes der vier aktionsauslösenden Tools (`propose_command`,
`read_remote_file`, `write_remote_file`, `propose_note_update`) endet
**immer** in einer Bestätigung in der App, auch wenn eine Allow-Regel das
Kommando sonst automatisch ausführen würde. Ein externes Tool ist eine
eigene Vertrauensgrenze. Die Regel ist fest, keine Einstellung. Sie
verschärft nur: Ein `Deny` der Filter-Engine bleibt `Deny` und wird nicht
zur Bestätigung herabgestuft.

`list_servers` und `get_server_notes` lesen nur gespeicherte Daten, bauen
keine Verbindung auf und brauchen keine Bestätigung.

## 6. Sicherheitszusagen

- **Nur `127.0.0.1`**, nie aus dem Netzwerk erreichbar.
- **Bearer-Token.** Jeder Aufruf ohne oder mit falschem Token wird
  abgelehnt, ohne Teil-Zugriff. Ein leeres oder nur aus Leerzeichen
  bestehendes Token gewährt nie Zugriff, auch nicht einem Client, der selbst
  ein leeres Token schickt. Findet die App beim Laden ein solches
  gespeichertes Token, ersetzt sie es durch ein neu erzeugtes; die
  Einstellungen zeigen danach wie gewohnt das neue Token, ohne zusätzlichen
  Dialog. Das Token liegt in der verschlüsselten Datenbank (Spec 0101).
- **Standardmäßig aus.** Der Server läuft nur, wenn der Nutzer ihn in den
  Einstellungen einschaltet. War er beim Beenden eingeschaltet, startet er
  beim nächsten App-Start (nach dem Entsperren) wieder.
- **Server-Allow-Liste.** Nur ausdrücklich ausgewählte Server sind über MCP
  ansprechbar, anfangs keiner. `list_servers` nennt nur diese. Jedes Tool
  mit `server_id` antwortet für einen Server außerhalb der Liste mit
  „unbekannter Server", genau wie für einen nicht existierenden — nie
  „Zugriff verweigert". So verrät die Antwort nicht, ob es den Server gibt.
- **Herkunft sichtbar.** Eine Aktionskarte aus MCP trägt ein
  Herkunfts-Abzeichen „Externes Tool (MCP)" mit dem Namen des Clients
  (§9a) statt des Namens des KI-Anbieters. Der Nutzer erkennt immer, ob
  eine Anfrage aus dem eigenen Chat oder von einem externen Agenten kommt.
- **Protokoll.** Jeder MCP-Tool-Aufruf und sein Ausgang wird über das
  strukturierte Logging (Spec 0016) mit `origin: "mcp"` protokolliert.

## 7. Warten auf Bestätigung (Timeout)

Ein aktionsauslösender Tool-Aufruf wartet, bis der Nutzer in der App
entscheidet, höchstens aber das eingestellte Timeout (Standard 5 Minuten).
Läuft es vorher ab, antwortet der Tool-Aufruf mit „Zeitüberschreitung beim
Warten auf Bestätigung — die Anfrage steht weiterhin in der App zur
Entscheidung offen." Die Aktionskarte in der App bleibt bestehen und kann
weiterhin bestätigt oder abgelehnt werden; nur der Tool-Aufruf ist beendet.
Für eine Entscheidung danach gibt es keine Rückmeldung an den Client.

## 8. Transport

Streamable HTTP, nicht stdio: Der externe Client verbindet sich mit der
bereits laufenden App und ihren Sitzungen, er startet sie nicht selbst. Der
Port ist fest `47823`. Endpunkt und Token trägt der Nutzer in die
MCP-Konfiguration seines Clients ein.

## 9. Einstellungen

Eigener Abschnitt „MCP-Server" in den Einstellungen:

- Schalter „MCP-Server aktivieren" (Standard aus).
- Endpunkt (`http://127.0.0.1:47823`) und Token, immer sichtbar. „Neu
  generieren" fragt vorher nach und macht das alte Token sofort ungültig,
  auch für einen laufenden Server und schon verbundene Clients.
- Timeout für das Warten auf Bestätigung in ganzen Minuten (mindestens 1).
  Eine Änderung startet einen laufenden Server neu, damit sie sofort gilt;
  ein gerade wartender Tool-Aufruf bricht dabei mit einem Verbindungsfehler
  ab, die Aktionskarte in der App bleibt.
- Mehrfachauswahl der Server auf der Allow-Liste.
- Beispiel-Konfiguration für einen Client. Sie wird vollständig angezeigt
  (umbrochen, ohne eigenen Scrollbereich) und lässt sich per Schaltfläche
  „Konfiguration kopieren" in die Zwischenablage kopieren, auch per
  Tastatur. Kopiert wird exakt der angezeigte Text; eine kurze Rückmeldung
  bestätigt das Kopieren oder meldet, dass es fehlgeschlagen ist. Der Text
  enthält das Token, die Zwischenablage danach ebenfalls (gleiche
  Offenlegung wie beim manuellen Markieren); die App protokolliert den Text
  nicht.

## 9a. Ablauf einer eingehenden MCP-Aktion

Ein externer Agent arbeitet oft, während die App im Hintergrund liegt. Eine
Anfrage darf deshalb nicht unbemerkt in den Timeout laufen.

- **Eigener Tab, eigene Verbindung.** Jede aktionsauslösende Anfrage läuft
  in der MCP-Sitzung ihres Clients für diesen Server, mit eigener
  SSH-Verbindung und eigenem Tab; fehlt sie, wird sie angelegt. Ein
  Nutzer-Tab desselben Servers wird nie verwendet. Der Tab erscheint, ohne
  den Fokus zu übernehmen, und trägt bei einer wartenden Bestätigung ein
  beschriftetes Abzeichen (Einzelheiten Spec 0104). Die Aktionskarte nennt
  den Server wie jede andere Bestätigung.
- **Keine manuelle Verbindung nötig.** Die Verbindung entsteht mit
  denselben gespeicherten Zugangsdaten und demselben Ablauf wie beim Klick
  auf den Server in der Seitenleiste. Ist der Host-Key unbekannt oder
  geändert, wartet der Aufbau auf die Host-Key-Entscheidung des Nutzers
  (Spec 0005), bevor die Aktion überhaupt zur Bestätigung erscheint. Der
  Host-Key-Dialog erscheint dabei über der ganzen App, unabhängig vom
  aktiven Tab. Solange der Hinweis beim ersten Start (Spec 0031) nicht
  bestätigt ist, scheitert der Verbindungsaufbau wie bei einem manuellen
  Verbindungsversuch.
- **OS-Benachrichtigung.** Sobald die Sitzung steht, zeigt die App eine
  native Benachrichtigung „Smart SSH: Bestätigung erforderlich" mit dem
  Text „<Client> möchte eine Aktion auf '<Server>' ausführen." (ohne
  Client-Namen „Ein externes Tool (MCP) …"). Kann die Benachrichtigung
  nicht gezeigt werden (z. B. keine Berechtigung), läuft die Aktion trotzdem
  normal weiter; der Client bekommt deswegen keinen Fehler.
- **Client-Name.** Übermittelt der Client beim Verbindungsaufbau einen
  Namen (`clientInfo.name`), nennen Abzeichen, Tab und Benachrichtigung ihn
  (gekürzt nach Spec 0104, §2); sonst steht dort die allgemeine
  Bezeichnung „externes Tool".
- **Nur die vier aktionsauslösenden Tools** legen Sitzung und Tab an und
  benachrichtigen. `list_servers` und `get_server_notes` bleiben still.

## 10. Prüfbare Fälle

- Jedes der vier aktionsauslösenden Tools wird zur passenden KI-Aktion und
  endet in einer Bestätigung, auch wenn eine Allow-Regel passt.
- Ein Server außerhalb der Allow-Liste ergibt „unbekannter Server", nicht
  „Zugriff verweigert"; das gilt auch für `get_server_notes`.
- Fehlendes, falsches oder leeres Token wird abgelehnt.
- Gegen einen echt gestarteten lokalen Server: Ein Tool-Aufruf über HTTP
  funktioniert, und nach Ablauf eines (kurzen) Timeouts kommt die
  Timeout-Antwort, während die Bestätigung offen bleibt.

## 11. Grenzen

- Keine Lockerung für einzelne Clients (etwa „diese Lese-Kommandos ohne
  Bestätigung"): Die Bestätigung aus §5 gilt ausnahmslos.
- Keine Tokens je Client und keine Allow-Liste je Client.
- Der Port ist nicht einstellbar.
- Ein Klick auf die OS-Benachrichtigung öffnet den MCP-Tab nicht gezielt;
  welches Fenster in den Vordergrund kommt, bestimmt das Betriebssystem.
- Während die Verbindung auf eine Host-Key-Entscheidung wartet, gibt es
  noch keine OS-Benachrichtigung; sichtbar ist dann nur der Host-Key-Dialog
  in der App.
