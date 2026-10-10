# Spec 0069 — Fünf-Minuten-Pfad: Einstieg und verständliche Fehler

Status: umgesetzt
Zweck: Ein Erstnutzer kommt von „Provider einrichten" über „Server anlegen" und „Verbindung testen" bis „verbinden und Frage stellen"; jeder Fehler auf diesem Weg nennt Ursache und nächsten Schritt, ein lokales Ollama wird gefunden und angeboten, und eine leere Installation führt in den Pfad.
Bezüge: Spec 0024 (Fehlercodes, i18n), Spec 0025 (Provider-Discovery), Spec 0031 (Erststart-Hinweis), Spec 0032 (lokaler Pseudo-Server), Spec 0033 (Server-Übersicht), Spec 0047 Fund D2 und ADR 0039 (kurze Meldung statt Rohtext), Spec 0050/0056 (Provider-Einstellungen, „Zugangsdaten testen"), Spec 0051 (Rate-Limit), Spec 0068 Teil 5a/5b (Discovery-Timeouts, Host-Key-Timeout), Spec 0072 (Erkennung „Modell nicht gefunden"), ADR 0062, ADR 0110 (Verbindungsschritte, Auflösung des ersten Hops), ADR 0115 (Zeitgrenzen je Phase und Hop), ADR 0123 „SFTP-Sitzungsabbruch erkennen".
Review-Priorität: NORMAL, für den Verbindungsaufbau (Abschnitt 3.A, A3) ERHÖHT

## Entscheidungen

- **E1 — Ollama-Suche nur auf Nutzeraktion.** Die Anfrage an
  `127.0.0.1:11434` läuft nur, wenn der Nutzer die Provider-Einstellungen
  öffnet (und noch kein Ollama-Provider existiert) oder auf „Erneut suchen"
  klickt. Nie beim Start, nie im Hintergrund.
- **E2 — Ein erkanntes Ollama ist nur ein Vorschlag.** Erst ein Klick des
  Nutzers legt den Provider an.
- **E3 — Neue Fehlercodes und Felder sind additiv.** Ein unbekannter Code fällt
  im Frontend auf den Rohtext zurück, nie auf eine leere Anzeige.

## 1. Geltungsbereich

Der Pfad umfasst: KI-Provider einrichten und testen, Server anlegen, Verbindung
testen, verbinden (einschließlich Host-Key-Rückfrage), Frage stellen. Die
Meldungen der fünf häufigsten Stolpersteine waren bereits kurz und übersetzt
(Spec 0047 D2); diese Spec unterscheidet die Ursachen, die dort noch in
einem Text zusammenfielen (Modell unbekannt, lokaler Dienst aus, Timeout,
Port zu, Name unbekannt, keine Route, Abbruch), und lässt keine Stelle des Pfads
einen rohen Backend-Text zeigen, solange ein Code vorliegt.

## 2. Ziel und Nicht-Ziele

**Ziel:** Jeder Fehler des Pfads erscheint in DE und EN mit Ursache und
nächstem Schritt. Ollama auf dem Standard-Port wird ohne Eingabe gefunden und
angeboten. Eine frische Installation führt aktiv zu „Ersten Server anlegen".

**Nicht-Ziele:**

- Fehlermeldungen außerhalb des Pfads (Filter-Codes, SFTP, Gruppen,
  MCP-Einstellungen) bleiben unverändert.
- Die Verdrahtung des Verbindungsaufbaus im Command selbst ist nicht mockbar;
  die Zeitgrenzen sind über eine eigene Hilfsfunktion testbar.
- Kein einblendbares „technisches Detail" unter der Meldung (ADR 0039); Details
  stehen im Log.
- Kein Ollama auf anderem Port oder Host, keine Umgebungsvariable, kein Ollama
  im Netzwerk: nur `127.0.0.1:11434`.
- Keine Erkennung, ob ein Ollama-Modell Tool-Calling kann (außer der Meldung
  aus A2a, wenn der Provider es ablehnt).
- Keine Schlüsselerzeugung in der App, keine Erkennung des Schlüsseltyps aus
  dem Inhalt.
- Kein Onboarding-Assistent: nur ein Einstiegs-Block in der Server-Liste.

Entfallene Kennungen: Teil 0 (0.1–0.7, Vorab-Klärungen der Umsetzung). Sie
werden nicht neu vergeben.

## 3. Verhalten

### 3.A Fehler verständlich, mit nächstem Schritt

**A1 — Register.** Jeder Code des Pfads (Tabelle 4.1) hat einen nicht leeren
DE- und EN-Text, der Ursache und einen konkreten nächsten Schritt nennt. In den
`errors.*`-Texten steht kein Platzhalter. Das Frontend führt die Codes in einer
festen Liste und kennt sie als bekannt.

**A2a — Modell ohne Tool-Unterstützung.** Antwortet der Provider auf einen
Aufruf mit Tools mit HTTP 400, Fehlertyp `invalid_request_error` und einer
Meldung, die auf „does not support tools" endet (gemessen bei Ollama), zeigt die
App `AI_MODEL_NO_TOOL_SUPPORT` statt der generischen Provider-Meldung. Andere
400er, 404 und jeder andere Status behalten ihre Einstufung. Der Fehler beendet
den Zug: kein Kommando wird ausgeführt oder vorgeschlagen. Die Voreinstellung
der Ollama-Karte (natives Tool-Calling an) bleibt.

**A2 — KI-Fehler unterscheiden.**

- Status 401/403 → Authentifizierung fehlgeschlagen, 429 → Rate-Limit
  (Status wird zuerst geprüft; ein Wort „model" im Body ändert daran nichts).
- Status 404 oder 400, deren Antwort als „Modell nicht gefunden" erkennbar ist
  (strukturiertes Fehlerfeld laut Spec 0072, hilfsweise ein Textbaustein aus den
  gemessenen Antworten von Ollama, OpenAI, Anthropic und OpenRouter) →
  `AI_MODEL_NOT_FOUND`. Ein 404 oder 400 ohne solche Merkmale (falsche
  Base-URL, HTML-Seite) und alle übrigen Status → `AI_PROVIDER_UNAVAILABLE`.
- Scheitert der Verbindungsaufbau und das Ziel ist eine Loopback-Adresse
  (`localhost`, `127.0.0.0/8`, `::1`) → `AI_LOCAL_PROVIDER_UNREACHABLE`. Gemeint
  ist die Adresse, nicht der Provider-Typ: dieselbe Meldung passt für andere
  lokale Server, und ein Ollama auf einem anderen Rechner gilt als entfernter
  Endpunkt.
- Scheitert der Aufbau zu einem nicht lokalen Ziel (DNS, abgelehnt, TLS) →
  `AI_NETWORK_ERROR`; ebenso ein Abbruch mitten im Stream.
- Keine Antwort innerhalb der Frist (90 Sekunden ohne Daten) → `AI_TIMEOUT`;
  der Logtext „Keine Antwort vom KI-Provider seit über N Sekunden" bleibt
  wörtlich.
- Wiederholt wird nur bei 429; die neuen Fehlerarten nie.

**A3 — SSH-Verbindungsfehler unterscheiden** (ERHÖHT). Für den Verbindungsaufbau
gilt (Tabelle 4.2):

- Abgelehnte Verbindung → `SSH_CONNECTION_REFUSED`; keine Route zum Ziel
  → `SSH_HOST_UNREACHABLE`; Abbruch während des Aufbaus (Reset, Abort,
  vorzeitiges Ende, Disconnect) → `SSH_CONNECTION_CLOSED`; jede andere
  E/A-Störung → `SSH_CONNECTION_FAILED`.
- Der erste Hop wird ausdrücklich aufgelöst, danach mit den aufgelösten
  Adressen verbunden (Auflösung und TCP als getrennte Schritte im
  Verbindungsprotokoll, ADR 0110). Scheitert die Auflösung →
  `SSH_HOST_NOT_FOUND`. Scheitert der erste Hop danach mit
  `SSH_CONNECTION_FAILED`, prüft der Aufbau nachträglich, ob der Name
  auflösbar ist; wenn nicht → `SSH_HOST_NOT_FOUND`.
- Der Host-Key wird immer unter dem Hostnamen gespeichert und geprüft, nie unter
  der aufgelösten IP. Die ausdrückliche Auflösung ändert das Ergebnis eines
  Versuchs nicht.
- **Abbruch einer laufenden Sitzung.** Dieselben Abbrüche bei einer Operation
  auf einer bereits aufgebauten Sitzung (Kommando, interaktive Shell, SFTP,
  Trennen) ergeben `SSH_SESSION_CLOSED`, nie `SSH_CONNECTION_CLOSED`, dessen
  Text vom Aufbau spricht. Alle übrigen Zuordnungen sind in beiden Fällen
  gleich. Bei SFTP gilt ein Fehler als Sitzungsabbruch, wenn der Server
  „keine Verbindung" oder „Verbindung verloren" meldet oder der SFTP-Kanal
  endet und zugleich die SSH-Sitzung beendet ist; endet nur der Kanal
  (z. B. weil sudo den erhöhten Start ablehnt), bleibt es `SSH_CHANNEL_ERROR`
  (ADR 0123 „SFTP-Sitzungsabbruch erkennen").
- **Zeitgrenzen je Phase und je Hop** (ADR 0115). Jeder Hop (Zielserver und
  jeder Jump-Host davor) hat eigene Grenzen:

  | Phase (je Hop) | umfasst | Grenze |
  |---|---|---|
  | Verbindung und Handshake | erster Hop: Namensauflösung, TCP, SSH-Handshake mit Host-Key-Prüfung; weitere Hops: Tunnel über den vorherigen Hop, SSH-Handshake mit Host-Key-Prüfung | 10 s |
  | Anmeldung | die konfigurierte Anmeldung an diesem Hop, einschließlich Berühren eines Hardware-Schlüssels oder Agent-Bestätigung | 60 s |

  - Ein Server, der annimmt, aber nie ein SSH-Banner schickt, scheitert nach
    10 s. Eine Anmeldung zwischen 10 s und 60 s bricht nicht ab.
  - Die Gesamtdauer wächst linear mit der Zahl der Hops. Als reines
    Sicherheitsnetz begrenzt zusätzlich die Summe aller Phasengrenzen plus eine
    weitere Handshake-Grenze den ganzen Versuch (ein Hop 80 s, zwei Hops
    150 s); im normalen Ablauf greift sie nie vor einer Phasengrenze.
  - Läuft eine Grenze ab, endet der Versuch immer mit `SSH_TIMEOUT`: nie
    verbunden, nie mit einer Host-Key-Rückfrage, ohne weitere Phase. Das
    Schritt-Protokoll markiert den laufenden Schritt mit `SSH_TIMEOUT`.
  - Das Warten auf die Host-Key-Entscheidung des Nutzers liegt außerhalb jeder
    Grenze: ein unbekannter oder geänderter Host-Key beendet den Versuch sofort
    mit der Rückfrage; danach gilt nur der Host-Key-Timeout (Spec 0068 Teil 5b).
    Nach „Vertrauen" beginnt ein neuer Versuch mit frischen Grenzen.
  - „Verbindung testen" und der Verbindungsaufbau über MCP nutzen dieselben
    Grenzen; der MCP-Client bekommt den Fehler statt eines hängenden Aufrufs,
    der Test zeigt „Timeout".

**A4 — Codes aus der App-Shell.**

- Kein aktiver Provider beim Verbinden → `AI_NO_ACTIVE_PROVIDER`.
- Host-Key abgelehnt → `SSH_HOST_KEY_NOT_TRUSTED`; Host-Key-Bestätigung
  abgelaufen → `SSH_HOST_KEY_CONFIRM_TIMEOUT`. Der Rohtext mit `host:port` bleibt
  als Fallback.
- „Modelle laden" liefert den Code des Fehlers mit.
- Das Ergebnis „Netzwerkfehler" der Verbindungsprüfung und das Ergebnis „nicht
  erreichbar" der Provider-Zugangsdatenprüfung tragen zusätzlich einen
  optionalen Code. Die Einteilung der Zugangsdatenprüfung in gültig,
  Authentifizierung fehlgeschlagen und nicht erreichbar bleibt.

**A5 — Anzeige.**

- „Zugangsdaten testen" bei „nicht erreichbar" mit bekanntem Code: die
  übersetzte Meldung statt „Provider nicht erreichbar: {Text}". Sonderfall
  `AI_RATE_LIMITED`: eigener Text (4.1), weil die Chat-Meldung „Nachricht
  erneut senden" hier nicht passt. Ohne Code: der bisherige Text mit Rohtext.
- „Modelle laden" fehlgeschlagen: unter dem Hinweis steht bei bekanntem Code der
  übersetzte Grund.
- Verbindungstest im Server-Formular: „Netzwerkfehler" mit bekanntem Code zeigt
  die übersetzte Meldung, ohne Code den bisherigen Text. „Timeout" und
  „Anmeldung abgelehnt" nennen den nächsten Schritt (4.1).

### 3.B Ollama-Erkennung

**B1 — Wann geprobt wird (E1).** Beim Öffnen der KI-Provider-Einstellungen,
sobald die Provider-Liste geladen ist und kein Provider vom Typ `ollama`
existiert; sowie bei „Erneut suchen". Kein Timer, kein Retry, kein späteres
Neuladen der Liste löst die Probe erneut aus, kein Aufruf außerhalb der
Einstellungen.

**B2 — Wie.** Über dieselbe Modellabfrage wie „Modelle laden", fest gegen
`http://127.0.0.1:11434/v1` (nicht `localhost`, damit kein IPv6-Umweg), mit
den Zeitgrenzen und dem Logging der Modellabfrage (Spec 0068 Teil 5a).

**B3 — Anzeige** (Karte oberhalb von „Provider hinzufügen"):

| Ergebnis | Anzeige |
|---|---|
| läuft noch | „Suche lokales Ollama …" (nicht blockierend) |
| Modelle gefunden | „Ollama läuft auf diesem Rechner." mit Modell-Auswahl (Standard: erstes Modell), Hinweis, ob es als aktiver Provider verwendet wird („Wird als aktiver Provider verwendet." bzw. „Wird als weiterer Provider angelegt (aktuell bleibt ein anderer aktiv).") und Buttons „Ollama übernehmen" und „Nein danke" (blendet die Karte bis zum nächsten Öffnen aus) |
| keine Modelle | „Ollama läuft, aber es ist noch kein Modell geladen." mit Anleitung `ollama pull <modell>` und „Erneut suchen" |
| `AI_LOCAL_PROVIDER_UNREACHABLE` | „Kein lokales Ollama gefunden (Standard-Port 11434)." mit Anleitung: von ollama.com installieren, starten, ein Modell laden, dann „Erneut suchen" |
| jeder andere Fehler (z. B. anderer Dienst auf dem Port) | keine Karte |

Die Anleitung „nicht gefunden" erscheint nur, wenn noch gar kein Provider
konfiguriert ist oder im Formular der Typ `ollama` gewählt ist; wer bereits z. B.
Anthropic nutzt, bekommt keinen Ollama-Hinweis aufgedrängt.

**B4 — Übernehmen (E2).** „Ollama übernehmen" legt einen Provider vom Typ
`ollama` an: Name „Ollama (lokal)", Base-URL `http://127.0.0.1:11434/v1`, das
gewählte Modell, Platzhalter-Key, übrige Formular-Voreinstellungen. Nur wenn
noch kein Provider aktiv ist, wird er aktiv gesetzt. Danach wird die Liste neu
geladen. Schlägt das Anlegen fehl, erscheint der Fehler wie bei jedem
Hinzufügen und nichts wird aktiv gesetzt. Ohne Klick wird nie gespeichert.

**B5 — Ollama ohne Key im Formular.** Für den Typ `ollama` ist das
API-Key-Feld nicht erforderlich (Zusatz „(bei Ollama nicht nötig)"), „Zugangsdaten
testen" ist ohne Key freigegeben, und Hinzufügen, Testen und Modelle laden
senden bei leerem Feld den Platzhalter `ollama-no-key`. Andere Typen bekommen
nie einen Platzhalter. Wählt der Nutzer den Typ `ollama` bei leerer Base-URL,
wird sie mit `http://127.0.0.1:11434/v1` vorbelegt (sichtbar, änderbar). Für das
Backend ist der Platzhalter ein normaler Key.

**B6 — Erster Provider aus dem Formular wird aktiv.**

1. Ist beim Speichern noch kein Provider aktiv, wird der neue nach dem Anlegen
   aktiv gesetzt; ist schon einer aktiv, bleibt dieser aktiv und der neue
   inaktiv. Ist die Provider-Liste beim Speichern nicht geladen (Laden läuft
   oder ist fehlgeschlagen), ist das unbekannt: der neue Provider bleibt
   inaktiv.
2. Solange kein Provider aktiv ist, steht über „Hinzufügen" der Hinweis „Wird
   als aktiver Provider verwendet, da noch keiner aktiv ist." (EN: „Will be used
   as the active provider, since none is active yet."). Er erscheint erst, wenn
   die Liste geladen ist.
3. Schlägt das Anlegen fehl: Fehler wie bisher, nichts aktiv gesetzt.
4. Schlägt nur das Aktiv-Setzen fehl: der Provider bleibt gespeichert und
   erscheint inaktiv in der neu geladenen Liste, das Formular wird geleert, der
   Fehler wird übersetzt angezeigt. Der Provider wird nicht gelöscht; „Aktiv
   setzen" aktiviert ihn.

Ein neuer Provider entsteht beim Anlegen immer inaktiv; das Aktiv-Setzen ist ein
eigener, nachgelagerter Schritt.

### 3.C Einstieg

**C1 — Leerer Zustand.** „Leer" heißt: kein echter Server (der lokale
Pseudo-Server zählt nie; Gruppen zählen nicht; ein nicht nutzbarer Server
zählt als vorhanden). Ist die Liste leer und kein Ladefehler angezeigt, steht
unter dem Localhost-Eintrag ein Block:

- Überschrift „Noch kein Server angelegt" und der Satz „Lege deinen ersten Server
  an, um dich per SSH zu verbinden.",
- Button „Ersten Server anlegen": wechselt zum Tab „Verwalten" mit geöffnetem
  Formular für einen neuen Server (ohne Notiz-Fokus),
- Zeile „Ohne Server ausprobieren: „Localhost" oben öffnet eine Sitzung auf
  diesem Rechner — mit denselben Bestätigungen wie auf einem Server."

Existieren Gruppen, steht der Gruppenbaum zusätzlich darunter. Sobald ein
echter Server existiert, verschwindet der Block. Auch „Lade Server…" und
„Verbinde…" sind übersetzt.

**C2 — Ed25519-Empfehlung.** Im Server-Formular steht bei der Anmeldeart
„Private Key" unter dem Schlüsselfeld: „Empfohlen: Ed25519-Schlüssel. Neu
erzeugen mit `ssh-keygen -t ed25519`. RSA-Schlüssel funktionieren weiterhin."
Nur Text; keine Prüfung des Inhalts, keine Sperre, kein Einfluss auf Speichern
oder Verbinden.

## 4. Design

### 4.1 Fehler-Register des Pfads (Texte DE / EN)

| Code | DE | EN |
|---|---|---|
| `AI_AUTH_FAILED` | Authentifizierung beim KI-Provider fehlgeschlagen – API-Key prüfen | Authentication with the AI provider failed – check the API key |
| `AI_MODEL_NOT_FOUND` | Der KI-Provider kennt dieses Modell nicht. Modellnamen in den Einstellungen prüfen („Modelle laden" zeigt die verfügbaren); bei Ollama das Modell zuerst mit `ollama pull <name>` laden. | The AI provider doesn't know this model. Check the model name in Settings ("Load models" lists the available ones); for Ollama, pull it first with `ollama pull <name>`. |
| `AI_MODEL_NO_TOOL_SUPPORT` | Dieses Modell unterstützt keine Tool-Aufrufe. Beim Provider in den Einstellungen „Natives Tool-Calling unterstützt" abwählen (dann läuft der Textmodus) oder ein Modell mit Tool-Unterstützung wählen. | This model does not support tool calling. In the provider settings, untick "Supports native tool calling" (the prompt-based fallback is used then) or pick a model with tool support. |
| `AI_LOCAL_PROVIDER_UNREACHABLE` | Der lokale KI-Dienst antwortet nicht. Ollama (bzw. deinen lokalen Server) starten und erneut versuchen. | The local AI service isn't responding. Start Ollama (or your local server) and try again. |
| `AI_NETWORK_ERROR` | Keine Verbindung zum KI-Provider. Internetverbindung und – bei eigenem Endpunkt – die Base-URL prüfen, dann erneut senden. | Can't reach the AI provider. Check your internet connection and, for a custom endpoint, the base URL, then send again. |
| `AI_TIMEOUT` | Der KI-Provider hat zu lange nicht geantwortet. Kurz warten und erneut senden; bei einem lokalen Modell: ist der Rechner ausgelastet oder das Modell zu groß? | The AI provider took too long to respond. Wait a moment and send again; with a local model, check whether the machine is busy or the model too large. |
| `AI_PROVIDER_UNAVAILABLE` | Der KI-Provider meldet einen Fehler (z. B. Wartung oder Überlastung). Später erneut versuchen; hält es an, Provider-Einstellungen prüfen. | The AI provider reported an error (e.g. maintenance or overload). Try again later; if it persists, check the provider settings. |
| `AI_RATE_LIMITED` | Der KI-Anbieter drosselt gerade die Anfragen (Rate-Limit) – die App hat es automatisch mehrfach erneut versucht. Bitte kurz warten und die Nachricht erneut senden. | The AI provider is currently throttling requests (rate limit) – the app already retried automatically a few times. Please wait a moment and send the message again. |
| `AI_NO_ACTIVE_PROVIDER` | Noch kein aktiver KI-Provider. In den Einstellungen unter „KI-Provider" einen Provider hinzufügen und aktiv setzen. | No active AI provider yet. Add one under Settings → AI provider and set it active. |
| `SSH_CONNECTION_REFUSED` | Der Server lehnt die Verbindung ab. Port prüfen und ob dort ein SSH-Dienst läuft. | The server refused the connection. Check the port and whether an SSH service is running there. |
| `SSH_HOST_NOT_FOUND` | Der Hostname ist unbekannt. Schreibweise der Adresse prüfen (oder die IP-Adresse eintragen). | Unknown host name. Check the address spelling (or enter the IP address). |
| `SSH_HOST_UNREACHABLE` | Der Server ist aus diesem Netz nicht erreichbar. Netzwerk/VPN prüfen. | The server can't be reached from this network. Check your network/VPN. |
| `SSH_TIMEOUT` | Der Server antwortet nicht. Adresse, Port und Firewall prüfen und erneut verbinden. | The server isn't responding. Check address, port and firewall, then connect again. |
| `SSH_CONNECTION_CLOSED` | Der Server hat die Verbindung während des Aufbaus beendet. Prüfen, ob auf diesem Port ein SSH-Dienst läuft; ggf. kurz warten (Schutz vor zu vielen Versuchen). | The server closed the connection during setup. Check that an SSH service runs on this port; wait a moment if too many attempts were made. |
| `SSH_SESSION_CLOSED` | Die Verbindung zum Server wurde unterbrochen. Erneut verbinden, um weiterzuarbeiten. | The connection to the server was lost. Reconnect to continue. |
| `SSH_CONNECTION_FAILED` | Verbindung fehlgeschlagen – ist der Host erreichbar (Adresse, Port, Netzwerk)? | Connection failed – is the host reachable (address, port, network)? |
| `SSH_AUTH_FAILED` | Anmeldung abgelehnt. Benutzername und Passwort bzw. Schlüssel in den Server-Einstellungen prüfen. | Login rejected. Check the user name and password or key in the server settings. |
| `SSH_HOST_KEY_NOT_TRUSTED` | Verbindung abgebrochen, weil der Host-Key nicht bestätigt wurde. Wenn du dem Server vertraust: erneut verbinden und den Fingerprint prüfen. | Connection cancelled because the host key wasn't confirmed. If you trust the server, connect again and verify the fingerprint. |
| `SSH_HOST_KEY_CONFIRM_TIMEOUT` | Die Host-Key-Abfrage wurde nicht rechtzeitig beantwortet; der Key wurde nicht übernommen. Erneut verbinden. | The host key prompt wasn't answered in time; the key was not trusted. Connect again. |

Weitere Texte im Pfad (keine `errors.*`-Codes):

- Zugangsdatenprüfung „Authentifizierung fehlgeschlagen": „✗ Authentifizierung
  fehlgeschlagen – API-Key prüfen" / „✗ Authentication failed – check the API
  key".
- Zugangsdatenprüfung bei Rate-Limit: „Zugangsdaten vermutlich gültig, der
  Anbieter drosselt aber gerade. In einer Minute erneut testen." / „Credentials
  look valid, but the provider is rate-limiting right now. Test again in a
  minute."
- Verbindungstest im Server-Formular: „✗ Anmeldung abgelehnt. Benutzername und
  Passwort bzw. Schlüssel prüfen." / „✗ Login rejected. Check the user name and
  password or key." sowie „✗ Der Server antwortet nicht. Adresse, Port und
  Firewall prüfen." / „✗ The server isn't responding. Check address, port and
  firewall." Ohne Code erscheint „✗ Netzwerkfehler: {Text}" /
  „✗ Network error: {text}".

Die Host-Key-Dialoge selbst bleiben unverändert; sie nennen bereits Ursache
und Entscheidung (Fingerprint prüfen, Hinweis auf Man-in-the-Middle).

### 4.2 Unterscheidung der Verbindungsfehler

**KI-Seite** (Entscheidung am Transportfehler, nicht am Provider-Typ):

| Situation | Erkennung | Code |
|---|---|---|
| lokaler Dienst nicht gestartet | Verbindungsaufbau scheitert, Ziel ist Loopback | `AI_LOCAL_PROVIDER_UNREACHABLE` |
| entfernter Provider nicht erreichbar (DNS, abgelehnt, TLS) | Verbindungsaufbau scheitert, kein Loopback | `AI_NETWORK_ERROR` |
| keine Antwort in der Frist | Inaktivitäts-Frist (90 s) oder Transport-Timeout | `AI_TIMEOUT` |
| Abbruch mitten im Stream | Stream-Fehler, nicht Verbindungsaufbau | `AI_NETWORK_ERROR` |

**SSH-Seite:**

| Situation | Erkennung | Code |
|---|---|---|
| Name nicht auflösbar | Auflösung des ersten Hops scheitert, oder `SSH_CONNECTION_FAILED` und nachträgliche Auflösung scheitert | `SSH_HOST_NOT_FOUND` |
| Port zu / kein Dienst | Verbindung abgelehnt | `SSH_CONNECTION_REFUSED` |
| keine Route | Host oder Netz nicht erreichbar | `SSH_HOST_UNREACHABLE` |
| keine Antwort | eine Phasengrenze (A3) abgelaufen oder Betriebssystem-Timeout | `SSH_TIMEOUT` |
| Abbruch während des Aufbaus | Disconnect, Reset, Abort, vorzeitiges Ende | `SSH_CONNECTION_CLOSED` |
| Abbruch einer aufgebauten Sitzung | dieselben Fehler bei Kommando, Shell, SFTP oder Trennen | `SSH_SESSION_CLOSED` |
| sonst | – | `SSH_CONNECTION_FAILED` |

Ein Server, auf dessen Port ein Nicht-SSH-Dienst läuft, endet je nach Antwort
des Dienstes in `SSH_CONNECTION_CLOSED` oder `SSH_CONNECTION_FAILED`.

### 4.3 Änderungen an den Schnittstellen

Alle additiv (E3): neue Codewerte in den Fehler-DTOs, ein optionales Feld `code`
in den beiden Ergebnissen aus A4. Kein neuer Command, keine Änderung an
Datenbank-Schema, Sync- oder Lizenzformat. Ein altes Frontend ignoriert das Feld;
ein unbekannter Code fällt auf den Rohtext zurück.

### 4.4 Provider-Unterschiede

- **Anthropic:** Modell-Fehler an der Antwortstruktur (Spec 0072); eine
  Ollama-Probe betrifft den Typ nicht.
- **OpenAI, OpenRouter, generische Endpunkte:** wie oben. OpenRouter meldet ein
  unbekanntes Modell mit 400; deshalb gilt 400 nur mit Modell-Merkmal.
- **Ollama:** Platzhalter-Key; lokal → `AI_LOCAL_PROVIDER_UNREACHABLE`. Ein
  Ollama auf einem anderen Rechner verhält sich wie ein entfernter Endpunkt.

### 4.5 Wechselwirkungen

| Mechanismus | Wirkung |
|---|---|
| Redaction | Keine neue Datensenke. Die Probe nutzt den Log-Pfad der Modellabfrage. Der Platzhalter ist kein Geheimnis, wird aber wie jeder Key aus Logs entfernt; deshalb ist es ein Wert, der in normalem Text nicht vorkommt (nicht „ollama"). |
| Rate-Limit, Wiederholung | Die Probe ist kein Chat-Aufruf. Wiederholt wird nur bei 429. |
| Fencing, Kompaktierung, Caching, Stopp/Einreihen, Ledger | nicht berührt (kein neuer KI-Aufruf, keine Aktion auf einem Server). |
| MCP | Die Zeitgrenzen gelten auch für den Verbindungsaufbau über MCP; der Client bekommt die Fehlermeldung. |
| Nie hängen | Zeitgrenzen je Phase und Hop; die Auflösung des ersten Hops und die Nachdiagnose laufen innerhalb der Handshake-Grenze. |
| Kein Aufruf ohne Nutzeraktion | Die Probe läuft nur nach E1, nur gegen Loopback, nie beim Start. |

## 5. Sicherheitszusagen

- **Host-Keys:** Kein Pfad dieser Spec akzeptiert einen Host-Key. Jede
  Zeitgrenze des Verbindungsaufbaus (je Phase, je Hop, das äußere
  Sicherheitsnetz) liefert immer einen Fehler, nie „verbunden", nie eine
  Vertrauensentscheidung, und umschließt nie das Warten auf die
  Host-Key-Entscheidung. Eine unbekannte oder geänderte Host-Key-Rückfrage wird
  nie als Verbindungsfehler fehlklassifiziert. Der Hostname, unter dem
  Host-Keys gespeichert und geprüft werden, bleibt unverändert.
- **Fehler erscheinen im UI:** Jede Fehlerart wird mit Code angezeigt; kein Pfad
  bricht still ab. Unbekannter Code → Rohtext, nie leere Anzeige.
- **Keine Secrets in Meldungen:** Die Nutzlasten der neuen SSH-Fehler enthalten
  nur Fehlertext der Verbindung bzw. den Hostnamen, nie ein Passwort; die
  KI-Nutzlasten haben denselben Inhalt wie zuvor. Bei bekanntem Code zeigt das
  Frontend die Nutzlast gar nicht (ADR 0039).
- **Credentials:** Der Ollama-Platzhalter geht über den normalen Weg zum
  Anlegen eines Providers; der Zugangsdaten-Speicher bleibt unverändert.
- **Filter-Engine, Risiko-Einstufung, Bestätigung/Auto-Ausführung, Redaction:**
  nicht berührt.
- **Kein Phone-Home:** Probe-Ziel fest `127.0.0.1:11434`, nur nach E1.
- **Keine stillen Rückfälle:** Ein Vorschlag wird nie ohne Klick übernommen
  (E2); ein fehlgeschlagenes Anlegen setzt nichts aktiv; ein über das Formular
  angelegter Provider wird nur aktiv, wenn vorher keiner aktiv war — ein
  aktiver Provider wird nie stillschweigend ersetzt (B6).

## 6. Testfälle

Jeder Regressionstest scheitert gegen den Stand vor der jeweiligen Änderung.

**Teil A, Rust**

1. „Modell nicht gefunden" mit den Fixtures von Ollama, OpenAI, Anthropic
   (404) und OpenRouter (400) → `AI_MODEL_NOT_FOUND`.
2. Negativ: 404 ohne Merkmal (falsche Base-URL, HTML), 400 ohne Merkmal →
   `AI_PROVIDER_UNAVAILABLE`; 401 mit „model" im Body → Authentifizierung.
3. Verbindung gegen `127.0.0.1:<freier Port>` → `AI_LOCAL_PROVIDER_UNREACHABLE`;
   gegen einen nicht auflösbaren Namen unter `.invalid` → `AI_NETWORK_ERROR`.
4. Timeout bei Discovery und Chat → `AI_TIMEOUT`, Logtext wörtlich wie zuvor.
5. Provider-Zugangsdatenprüfung mit Rate-Limit, Modell unbekannt, lokaler
   Dienst aus → „nicht erreichbar" mit dem jeweiligen Code.
6. Jede E/A-Art der Tabelle 4.2 und Disconnect → erwarteter SSH-Code; unbekannte
   Art → `SSH_CONNECTION_FAILED`; im Sitzungskontext `SSH_SESSION_CLOSED`.
7. Integration: geschlossener lokaler Port → `SSH_CONNECTION_REFUSED`;
   `name.invalid` → `SSH_HOST_NOT_FOUND`.
8. Verbindungstest: „Netzwerkfehler" trägt den Code.
9. Kein aktiver Provider → `AI_NO_ACTIVE_PROVIDER`.
10. Die Codes aller drei Fehlerfamilien sind eindeutig.

**Teil A3, adversarial (ERHÖHT)**

11. Nie fertig werdende Verbindung → `SSH_TIMEOUT`, null Vertrauensaufrufe.
12. Sofortige Host-Key-Rückfrage, danach vergeht mehr als jede Grenze → kein
    `SSH_TIMEOUT`; es gilt nur der Host-Key-Timeout.
13. Server, der annimmt und nie ein Banner schickt → `SSH_TIMEOUT` an der
    Handshake-Grenze, null Vertrauensaufrufe.
13a. Grenzen je Phase und Hop (pausierte Uhr): hängender Handshake →
    `SSH_TIMEOUT` ohne Anmeldung; Anmeldung zwischen den Grenzen → verbunden;
    Anmeldung über der Grenze → `SSH_TIMEOUT`; zwei Hops je knapp unter den
    Grenzen → verbunden; Ablauf am zweiten Hop → `SSH_TIMEOUT` ohne weitere
    Phase; Verbindungstest meldet „Timeout"; das Sicherheitsnetz wächst mit der
    Hop-Zahl.
14. Unbekannter Key → weiter Host-Key-Rückfrage (nicht als Verbindungsfehler);
    bekannter Key unter dem Hostnamen → verbunden.
15. Geänderter Key → weiter Rückfrage „Mismatch", nie ein Verbindungsfehler-Code.
16. Erfolgreicher Connect löst keine Namensauflösung über die des ersten Hops
    hinaus aus.
17. Connect mit Passwort gegen geschlossenen Port: weder Meldung noch Log
    enthalten das Passwort.

**Teil A, Frontend**

18. Jeder Code des Registers ist bekannt, hat DE- und EN-Text, nicht leer, DE ≠ EN.
19. „Zugangsdaten testen": `AI_RATE_LIMITED` → eigener Text; `AI_MODEL_NOT_FOUND`
    → übersetzter Text; ohne Code → bisheriger Text mit Rohtext.
20. Verbindungstest: `SSH_CONNECTION_REFUSED` → übersetzt; ohne Code → bisheriger
    Text.

**Teil B**

21. Mount ohne Ollama-Provider → genau eine Probe gegen `127.0.0.1:11434/v1`;
    mit Ollama-Provider → keine; „Erneut suchen" → eine weitere.
22. Start der App ohne geöffnete Einstellungen löst keine Probe aus (gilt
    strukturell: die Probe lebt nur in den Provider-Einstellungen).
23. Ergebnisse: Modelle → Vorschlag; leer → Pull-Anleitung;
    `AI_LOCAL_PROVIDER_UNREACHABLE` → Installationsanleitung (nur ohne Provider
    oder bei Typ `ollama`); anderer Fehler → keine Karte.
24. „Ollama übernehmen": ohne aktiven Provider → anlegen, dann aktiv setzen;
    mit aktivem → nicht aktiv setzen; Anlegen scheitert → nichts aktiv.
25. Ohne Klick wird nie ein Provider angelegt.
26. Platzhalter nur bei Typ `ollama` und leerem Key; Wert bleibt Wert; andere
    Typen bleiben leer.
26a. Formular (B6): ohne aktiven Provider → Hinweis, Anlegen und Aktivieren;
    mit aktivem → kein Hinweis, kein Aktivieren; Anlegen scheitert → Fehler,
    nichts aktiv; Aktivieren scheitert → übersetzter Fehler, Provider inaktiv in
    der Liste, nicht gelöscht; Liste nicht geladen → angelegt, nicht aktiviert.

**Teil C**

27. Nur Localhost → Block sichtbar; ein echter Server → kein Block; nur Gruppen →
    Block und Gruppenbaum.
28. „Ersten Server anlegen" öffnet das Formular für einen neuen Server.
29. Anmeldeart „Private Key" → Ed25519-Hinweis sichtbar; andere Anmeldearten →
    nicht.

## 7. Grenzen

- Die ausdrückliche Auflösung betrifft nur den ersten Hop; ab dem zweiten Hop
  gibt es keine eigene Namensauflösungs-Diagnose.
- „Modell nicht gefunden" ist an der Antwortstruktur des Providers erkannt; ein
  404 desselben Typs für eine andere Ressource (z. B. falsche Attestierungs-URL
  bei Anthropic) kann als „Modell nicht gefunden" erscheinen. Das betrifft nur
  den Meldungstext (Spec 0072).
- Die Sekundenzahl bei einem Transport-Timeout der KI ist eine Näherung; die
  Oberfläche zeigt ohnehin nur den Text ohne Zahl.
- Ein Nicht-SSH-Dienst auf dem Port ergibt je nach Antwort
  `SSH_CONNECTION_CLOSED` oder `SSH_CONNECTION_FAILED`.
- Ollama wird nur auf `127.0.0.1:11434` gesucht; andere Port- oder
  Host-Konfigurationen müssen manuell eingetragen werden.
- Ein Ollama-Modell ohne Tool-Unterstützung wird erst beim ersten Chat-Aufruf
  erkannt (A2a), nicht bei der Probe.
