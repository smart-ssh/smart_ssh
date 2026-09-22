# Spec: Fünf-Minuten-Pfad — Einstieg und verständliche Fehler

Status: Vorschlag (Architekt)
Backlog: BL-0013 (führend), BL-0153, BL-0036, BL-0082, BL-0110
Gate: pre-release-0x/D (BL-0013, BL-0082), release-1.0/B (BL-0036, BL-0082,
Messlatte für BL-0013), release-1.0/C (BL-0110, SOLL)
Repo: **öffentlich** `smart_ssh` — `crates/core` (Fehlertypen),
`crates/ai-providers` (Fehler-Mapping), `crates/ssh-transport`
(Fehler-Klassifizierung beim Verbindungsaufbau), `crates/app-shell`
(Commands, Connect-Timeout), Frontend (Fehler-Übersetzung, Provider-
Einstellungen, Server-Liste, Server-Formular, Locale-Dateien)
Abhängigkeiten: i18n/Fehler-Codes (0024), Pre-Release-Härtung Fund D2 +
ADR 0039, Settings/Testen-Button (0050/0056), Rate-Limit-Meldung (0051),
Provider-Discovery (0025), Timeouts in discovery.rs (0068 Teil 5a),
Host-Key-Timeout (0068 Teil 5b), lokaler Pseudo-Server (0032), gruppierte
Server-Übersicht (0033), Erststart-Hinweis (0031)
Review-Priorität: **NORMAL**, für **Teil A3 ERHÖHT** (berührt den
Verbindungsaufbau samt Host-Key-Schleife in `connect_session`)

> Drei Teile, gebündelt, weil sie denselben Weg betreffen: den ersten
> Kontakt eines Nutzers mit der App.
> **Teil A — Fehler verständlich, mit nächstem Schritt** (BL-0013, BL-0153):
> Jeder Fehler auf dem Fünf-Minuten-Pfad hat einen eigenen Code und eine
> DE/EN-Meldung, die Ursache **und** nächsten Schritt nennt.
> **Teil B — Ollama-Erkennung** (BL-0036): Beim Öffnen der Provider-
> Einstellungen sucht die App Ollama auf dem Standard-Port und schlägt es
> vor; fehlt es, steht dort eine Anleitung.
> **Teil C — Einstieg** (BL-0082, BL-0110): Ohne eigenen Server führt die
> Liste aktiv zu „Ersten Server anlegen"; beim Schlüssel-Login steht eine
> Ed25519-Empfehlung. C2 (BL-0110) ist unabhängig und einzeln abtrennbar.

---

## Getroffene Entscheidungen (Stefan, 2026-09-22)

- **E1 — Ollama-Probe nur auf Nutzeraktion.** Die Anfrage an
  `127.0.0.1:11434` läuft nur, wenn der Nutzer die Provider-Einstellungen
  öffnet bzw. dort ausdrücklich „Erneut suchen" klickt — nie beim App-Start,
  nie im Hintergrund, nie periodisch. Hält BL-0042 („Kein Netzwerkaufruf
  ohne Nutzeraktion") ein.
- **E2 — Erkanntes Ollama ist nur ein Vorschlag.** Die App setzt Ollama nie
  selbst als Provider. Erst ein Klick des Nutzers auf den Vorschlag legt den
  Provider an.
- **E3 — Neue Fehlercodes sind zugelassen.** Neue Codes in den Fehler-DTOs
  der Tauri-Commands sind erlaubt, sofern additiv; ein unbekannter Code
  fällt im Frontend wie bisher auf den Rohtext zurück (`translateErrorCode`).

Alles andere in dieser Spec ist ein **Vorschlag des Architekten**, der mit
Tor 1 freigegeben wird. Zwei Punkte, die über E1–E3 hinausgehen könnten,
stehen in §8.

---

## 1. Ausgangslage (belegt)

**Das Backlog ist hier teilweise überholt.** Spec 0047, Fund D2 hat die fünf
Meldungen bereits einmal nachgeschärft (ADR 0039: kurze übersetzte Meldung
statt Rohtext, keine technischen Details). Diese Spec baut darauf auf und
schließt nur die verbliebenen Lücken:

| Fall | Heute sichtbar | Lücke |
|---|---|---|
| Falscher Key (Chat) | `errors.AI_AUTH_FAILED` „… – API-Key prüfen" | keine (bleibt) |
| Falscher Key (Testen-Button) | `aiProvider.testResultAuthFailed` | nennt keinen nächsten Schritt |
| Modell nicht gefunden | `AI_PROVIDER_UNAVAILABLE` „… Provider-Konfiguration und Modellname prüfen" | nicht unterscheidbar von 5xx/Wartung; `crates/ai-providers/src/error.rs::map_http_status` wirft alles außer 401/403/429 auf `ProviderUnavailable` |
| Ollama nicht gestartet | `AI_NETWORK_ERROR` „… läuft er (z. B. Ollama gestartet)?" | derselbe Text für **jeden** Provider (Anthropic-Nutzer lesen „Ollama"); lokal/entfernt nicht unterschieden (`error.rs::map_transport_error` → immer `NetworkError`) |
| KI-Timeout | `AI_NETWORK_ERROR` (s. o.) | Timeout ist als `NetworkError("Keine Antwort … seit über 90 Sekunden")` an fünf Stellen gebaut (`anthropic.rs` ×2, `openai_compatible.rs` ×2, `discovery.rs::discovery_timeout`), vom Verbindungsfehler nicht unterscheidbar |
| Testen-Button, alle Nicht-Auth-Fehler | `aiProvider.testResultUnreachable` + **roher** `err.to_string()` inkl. HTTP-Body (`commands.rs::classify_credential_test_result`) | BL-0153: 429, Modell-Fehler usw. laufen nicht über `translateErrorCode` |
| Host nicht erreichbar | `SSH_CONNECTION_FAILED` „… ist der Host erreichbar (Adresse, Port, Netzwerk)?" | `ssh-transport/src/error.rs::map_russh_error` faltet **jeden** `io::Error` in `ConnectionFailed(String)` — abgelehnt, DNS, keine Route: alles gleich (ADR 0039, Konsequenzen) |
| Host antwortet nicht | wartet auf den OS-TCP-Timeout (je nach OS ca. 20 s bis > 2 min) | `commands.rs::connect_session` ruft `ssh_transport::connect` **ohne** Timeout auf; `SshError::Timeout` wird nirgends erzeugt (nur `test_connection.rs` hat 10 s) |
| Server-Formular „Verbindung testen", Netzwerkfehler | `serverForm.testResult.networkError` + **roher** Text | `TestConnectionResult::NetworkError { message }` trägt keinen Code |
| Host-Key unbekannt/geändert | `HostKeyDialog.tsx` (zwei Varianten, Portal) | Dialoge gut; aber **Ablehnen** und **Bestätigungs-Timeout** enden in rohen deutschen Strings ohne Code (`connect_session`, Zweige `HostKeyUserDecision::Reject` / `HostKeyWait::TimedOut`) |
| Kein aktiver KI-Provider beim Verbinden | roher Text „kein aktiver AI-Provider konfiguriert — …" (`active_ai_provider_config`), ohne Code | englische UI zeigt Deutsch |
| Leere Serverliste | `ServerList.tsx`: fest verdrahteter deutscher Entwicklertext („… s. `profiles_demo`-Beispiel oder CLI-Helfer …") | BL-0082; dazu „Lade Server…" / „Verbinde…" fest deutsch |
| Ollama-Provider anlegen | API-Key-Feld ist `required`; „Zugangsdaten testen" ist ohne Key gesperrt; `discover_models`/`test_ai_provider_credentials` lehnen leeren Key ab | Ollama braucht keinen Key — der Nutzer muss einen erfinden |
| Neuer Provider | wird inaktiv angelegt (`ai_provider_store.rs::create` ignoriert `is_active`) | nach „Hinzufügen" muss man noch „Aktiv setzen" — s. §8, Punkt 2 |

`CommandError` (`app-shell/src/error.rs`) trägt `code: Option<&'static str>`;
nur Stellen mit `CommandError::with_code` liefern einen Code. Der blanket
`From<E: Display>` verwirft ihn — das betrifft u. a. `discover_models`
(`ai_providers::discover_models(...).await?`).

---

## 2. Ziel und Nicht-Ziele

**Ziel:** Jeder Fehler, den ein Erstnutzer auf dem Pfad
*Provider einrichten → Server anlegen → Verbindung testen → verbinden →
Frage stellen* auslösen kann, erscheint in DE und EN mit **Ursache und
nächstem Schritt**. Ollama auf dem Standard-Port wird ohne Eingabe gefunden
und angeboten. Eine frische Installation führt aktiv in den Pfad.

**Nicht-Ziele (nicht mitreparieren):**
- Fehlermeldungen **außerhalb** des Pfads (Filter-Codes, SFTP, Gruppen,
  MCP-Einstellungen …) — bleiben unverändert.
- Ein einblendbares „technisches Detail" unter der Meldung (ADR 0039 bleibt
  gültig; die Logdatei ist die Quelle für Details).
- Ollama auf anderem Port/Host, `OLLAMA_HOST`, Ollama im Netzwerk. Nur
  `127.0.0.1:11434`.
- Automatische Erkennung, ob ein Ollama-Modell Tool-Calling kann (nur
  Teil-0-Bericht, §3.0.4).
- Schlüssel-Erzeugung in der App; Erkennung des Schlüsseltyps aus dem
  eingefügten Inhalt.
- Verlinkung von ADR 0028 in einem THREAT-MODEL (BL-0049/BL-0126).
- Mockbarkeit von `connect_session` (BL-0146). Der neue Timeout wird über
  eine kleine Hilfsfunktion getestet (§6).
- Fehler innerhalb einer Jump-Host-Kette ab dem zweiten Hop (bleiben
  wie heute).
- Ein Onboarding-Assistent / Wizard. Nur ein Einstiegs-Block in der Liste.

---

## 3. Anforderungen

### 3.0 Teil 0 — vom Coder zuerst zu klären (berichten, dann bauen)

Jede Antwort kommt als kurzer Bericht **vor** dem jeweiligen Teil. Ergibt
sich ein Widerspruch zur Spec: melden, nicht raten.

1. **Toolchain:** Ist `std::io::ErrorKind::HostUnreachable` /
   `NetworkUnreachable` mit der Workspace-Toolchain verfügbar (stabil seit
   Rust 1.83)? Falls nein: melden; dann entfällt `SSH_HOST_UNREACHABLE` und
   der Fall bleibt `SSH_CONNECTION_FAILED`.
2. **Modell-nicht-gefunden-Antworten** (externe Fakten, **zu verifizieren**,
   nicht aus dem Gedächtnis): Status und Body bei unbekanntem Modellnamen für
   Ollama (OpenAI-kompatibler Endpunkt), OpenAI, Anthropic, OpenRouter. Aus
   Provider-Doku oder einem echten Aufruf; als Test-Fixtures ablegen
   (ohne Keys). Erwartung des Architekten, ungeprüft: Ollama/OpenAI/Anthropic
   404, OpenRouter 400.
3. **Ollama-Fakten** (zu verifizieren): `GET http://127.0.0.1:11434/v1/models`
   liefert die installierten Modelle im OpenAI-Format (`{"data":[{"id":…}]}`);
   der lokale Ollama-Server ignoriert einen `Authorization`-Header; Chat über
   `…/v1/chat/completions` funktioniert mit beliebigem Bearer-Wert.
4. **Ollama-Modell ohne Tool-Calling** (nur berichten): Welche Antwort kommt,
   wenn ein Modell ohne Tool-Unterstützung mit `tools` angefragt wird, und
   welche Meldung sieht der Nutzer heute? Kein Fix in dieser Spec — der
   Architekt entscheidet danach, ob ein Folge-Item nötig ist.
5. **`reqwest`-Klassifizierung:** Liefert ein abgelehnter Verbindungsaufbau
   (`127.0.0.1:<freier Port>`) `is_connect() == true` und `err.url()` mit
   dem Ziel-Host? Was liefert ein DNS-Fehler, was ein TLS-Fehler? Mit einem
   Unit-Test belegen.
6. **Nicht-SSH-Port:** Was liefert `ssh_transport::connect`, wenn auf dem Port
   ein anderer Dienst läuft (z. B. ein lokaler HTTP-Listener)? Nur berichten;
   nur wenn die `russh`-Variante eindeutig ist, fällt sie unter
   `SSH_CONNECTION_CLOSED` (§3.A3).
7. **Navigation „Ersten Server anlegen":** Funktioniert
   `ManagementView`s `initialSelection` mit `{ kind: "newServer" }`, und
   löst das **nicht** den Notiz-Fokus aus (`focusNotesOnOpen` wird heute aus
   `Boolean(initialSelection)` abgeleitet)? Falls doch: Fokus nur bei
   Server-Auswahl, nicht bei `newServer`.

### 3.A Teil A — Fehlercodes und Übersetzung (BL-0013, BL-0153)

**A1 — Pfad-Register.** Im Frontend gibt es eine exportierte, feste Liste
`FIVE_MINUTE_PATH_ERROR_CODES` (in `errorCodes.ts`) mit genau den Codes der
Tabelle in §4.1. Für jeden Code MUSS gelten:
- er steht in `KNOWN_ERROR_CODES`,
- `locales/de/common.json` und `locales/en/common.json` haben
  `errors.<CODE>`, nicht leer,
- der Text nennt **Ursache und einen konkreten nächsten Schritt** (Verb im
  Imperativ bzw. „prüfe/starte/lege an …"). Diese Bedingung prüft der
  Reviewer am Text; der Test prüft Vorhandensein.

**A2 — KI-Fehler unterscheiden** (`crates/core/src/ai/types.rs`,
`crates/ai-providers/src/error.rs`):
- `AiError` bekommt drei neue Varianten mit Codes:
  `ModelNotFound(String)` → `AI_MODEL_NOT_FOUND`,
  `LocalProviderUnreachable(String)` → `AI_LOCAL_PROVIDER_UNREACHABLE`,
  `Timeout { secs: u64 }` → `AI_TIMEOUT`.
- `map_http_status`: Status 404 **oder** 400 **und** der Body enthält
  (case-insensitive) einen der in Teil 0.2 belegten Modell-Marker →
  `ModelNotFound`. Alles andere unverändert (401/403 → Auth, 429 →
  RateLimited, Rest → `ProviderUnavailable`). Die Marker stehen als eine
  Konstante in `error.rs` mit Verweis auf die Fixtures.
- `map_transport_error`: `err.is_connect()` **und** Host von `err.url()` ist
  Loopback (`localhost`, `127.0.0.0/8`, `::1`) → `LocalProviderUnreachable`.
  `err.is_timeout()` → `Timeout`. Alles andere → `NetworkError` wie bisher.
- Die fünf heutigen Timeout-Konstruktionen („Keine Antwort vom KI-Provider
  seit über N Sekunden") erzeugen `AiError::Timeout { secs }`; der
  `Display`-Text bleibt wörtlich derselbe (Log-Kontinuität). Eine
  Konstruktor-Funktion statt fünf Kopien.
- Retry-Verhalten unverändert: `retry.rs` wiederholt nur HTTP 429; die neuen
  Varianten werden nie wiederholt.

**A3 — SSH-Verbindungsfehler unterscheiden** (**ERHÖHT**)
(`crates/core/src/ssh/error.rs`, `crates/ssh-transport/src/error.rs`,
`crates/ssh-transport/src/connect.rs`, `crates/app-shell/src/commands.rs`):
- `SshError` bekommt neue Varianten mit Codes:
  `ConnectionRefused(String)` → `SSH_CONNECTION_REFUSED`,
  `HostNotFound(String)` → `SSH_HOST_NOT_FOUND`,
  `HostUnreachable(String)` → `SSH_HOST_UNREACHABLE` (entfällt ggf. nach 0.1),
  `ConnectionClosed(String)` → `SSH_CONNECTION_CLOSED`.
  `SshError::Timeout` → `SSH_TIMEOUT` besteht schon und wird jetzt erzeugt.
- `map_russh_error`, Zweig `russh::Error::IO(io_err)`, nach `io_err.kind()`:
  `ConnectionRefused` → `ConnectionRefused`; `TimedOut` → `Timeout`;
  `HostUnreachable`/`NetworkUnreachable` → `HostUnreachable`;
  `ConnectionReset`/`ConnectionAborted`/`UnexpectedEof` → `ConnectionClosed`;
  sonst `ConnectionFailed` wie bisher. `russh::Error::Disconnect` →
  `ConnectionClosed`.
- **DNS nachträglich diagnostizieren, nicht vorab:** Nur wenn der **erste**
  Hop mit `ConnectionFailed` scheitert, prüft `connect()` per
  `tokio::net::lookup_host((host, port))`, ob der Name überhaupt auflösbar
  ist; wenn nicht → `HostNotFound`. Der Erfolgspfad und der Host-Key-Pfad
  (`resolve_or_pending`, `ClientHandler { host, … }`) bleiben **byte-gleich**
  — keine Vorab-Auflösung, kein Ersetzen des Hostnamens durch eine IP.
- **Connect-Timeout in `connect_session`:** Jeder Aufruf von
  `ssh_transport::connect` in der Schleife läuft unter
  `tokio::time::timeout(SSH_CONNECT_TIMEOUT, …)`; Ablauf →
  `SshError::Timeout` → `CommandError::with_code(…, "SSH_TIMEOUT")`.
  `SSH_CONNECT_TIMEOUT` ist **die bestehende** 10-s-Konstante aus
  `test_connection.rs` (`TEST_CONNECTION_TIMEOUT`), umbenannt und an eine
  gemeinsame Stelle verschoben, von beiden Pfaden genutzt — keine zweite
  Konstante. Der Timeout umschließt **nur** `ssh_transport::connect`, nie das
  Warten auf die Host-Key-Entscheidung (das bleibt bei
  `PENDING_ACTION_CONFIRM_TIMEOUT`, Spec 0068 Teil 5b).
- Gilt für alle Aufrufer von `connect_session`, also auch für MCP
  (`mcp_backend::ensure_session`): der MCP-Client bekommt den Fehler statt
  eines hängenden Tool-Calls.

**A4 — App-Shell-Fehler mit Code** (`crates/app-shell/src/commands.rs`,
`error.rs`):
- `active_ai_provider_config` → `CommandError::with_code(…, "AI_NO_ACTIVE_PROVIDER")`.
- `connect_session`, Host-Key abgelehnt → `SSH_HOST_KEY_NOT_TRUSTED`;
  Host-Key-Bestätigung abgelaufen → `SSH_HOST_KEY_CONFIRM_TIMEOUT`.
  Die `message` (roher Fallback, enthält `host:port`) bleibt wie heute.
- `discover_models`: Fehler aus `ai_providers::discover_models` gehen mit
  `err.code()` raus (`CommandError::with_code`), statt über den blanket
  `From`.
- `test_connection`: `TestConnectionResult::NetworkError` bekommt ein
  zusätzliches Feld `code: Option<&'static str>` (serialisiert als `code`),
  gesetzt aus `SshError::code()`.
- `test_ai_provider_credentials` (**BL-0153**):
  `TestAiProviderCredentialsResult::Unreachable` bekommt ein zusätzliches
  Feld `code: Option<&'static str>`, gesetzt aus `AiError::code()`.
  Die dreiwertige Einteilung (valid / authenticationFailed / unreachable)
  bleibt unverändert.
- Alle neuen `CommandError`-Codes stehen in
  `error.rs::code_tests::test_command_error_with_code_values_are_unique`.
  Die neuen `SshError`/`AiError`-Varianten stehen in den jeweiligen
  Eindeutigkeitstests.

**A5 — Frontend** (`errorCodes.ts`, `AiProviderSettings.tsx`,
`ServerForm.tsx`, `ServerList.tsx`, Locales):
- Alle neuen Codes in `KNOWN_ERROR_CODES` und in beiden Locales.
- Testen-Button: bei `unreachable` mit bekanntem Code zeigt die Box
  `translateErrorCode(t, code, message)` statt „Provider nicht erreichbar:
  {{message}}". **Sonderfall 429:** `AI_RATE_LIMITED` zeigt im Testen-Kontext
  den eigenen Text `aiProvider.testResultRateLimited` (die Chat-Meldung sagt
  „Nachricht erneut senden" — passt hier nicht). Ohne Code: bisheriges
  Verhalten.
- `testResultAuthFailed` nennt den nächsten Schritt (Text §4.1).
- „Modelle laden" fehlgeschlagen: Unter dem bisherigen Hinweis steht der
  übersetzte Grund, wenn der Fehler einen bekannten Code trägt.
- `TestResultBadge` (Server-Formular): `networkError` mit bekanntem Code →
  übersetzte Meldung; ohne Code → wie bisher.
- `timeout` im `TestResultBadge` und `authFailed` bekommen Texte mit
  nächstem Schritt (§4.1).
- Keine Stelle im Pfad zeigt nach dieser Spec einen rohen Backend-Text,
  solange ein Code vorliegt.

### 3.B Teil B — Ollama-Erkennung (BL-0036)

**B1 — Wann geprobt wird (E1).** `AiProviderSettings` startet die Probe
genau dann, wenn
- die Komponente gemountet wird (= Nutzer öffnet Einstellungen → KI-Provider;
  das ist die Standard-Kategorie) **und** die Provider-Liste geladen ist
  **und** kein Provider vom Typ `ollama` existiert, **oder**
- der Nutzer auf „Erneut suchen" klickt.
Kein weiterer Auslöser. Kein Timer, kein Retry-Loop, kein Aufruf außerhalb
dieser Komponente.

**B2 — Wie geprobt wird.** Über den **bestehenden** Command
`discover_models` mit `{ providerType: "ollama", baseUrl:
"http://127.0.0.1:11434/v1", apiKey: OLLAMA_PLACEHOLDER_API_KEY }` — kein
neuer Tauri-Command, kein neuer HTTP-Pfad. Timeout und Logging sind die aus
`discovery.rs` (Spec 0068 Teil 5a). `127.0.0.1` statt `localhost`, damit
kein IPv6-Umweg nötig ist.

**B3 — Was angezeigt wird** (Karte oberhalb von „Provider hinzufügen"):

| Ergebnis | Anzeige |
|---|---|
| läuft noch | „Suche lokales Ollama …" (nicht blockierend) |
| `Ok(models)`, `models` nicht leer | **Vorschlag:** „Ollama läuft auf diesem Rechner." + Modell-Auswahl (Standard: erstes Modell) + Button „Ollama übernehmen" + Link-Button „Nein danke" (blendet die Karte bis zum nächsten Öffnen aus) |
| `Ok([])` | „Ollama läuft, aber es ist noch kein Modell geladen." + Anleitung `ollama pull <modell>` + „Erneut suchen" |
| Fehler mit Code `AI_LOCAL_PROVIDER_UNREACHABLE` | **Anleitung:** „Kein lokales Ollama gefunden (Standard-Port 11434)." — installieren (Download-Adresse von ollama.com als Text), starten, ein Modell laden, „Erneut suchen". Kompakt, einklappbar. |
| jeder andere Fehler (z. B. anderer Dienst auf dem Port) | keine Karte (still) |

Die Anleitung bei „nicht gefunden" erscheint nur, wenn **gar kein** Provider
konfiguriert ist oder im Formular der Typ `ollama` gewählt ist — wer bereits
Anthropic nutzt, bekommt keinen Ollama-Hinweis aufgedrängt.

**B4 — Übernehmen (E2).** Klick auf „Ollama übernehmen" ist die Bestätigung
des Nutzers. Dann:
1. `addAiProvider` mit Typ `ollama`, Name „Ollama (lokal)", Base-URL
   `http://127.0.0.1:11434/v1`, gewähltes Modell, Key
   `OLLAMA_PLACEHOLDER_API_KEY`, sonst Formular-Standardwerte;
2. **nur wenn noch kein Provider aktiv ist:** `setActiveAiProvider` auf den
   neuen Provider. Die Karte sagt das vor dem Klick („… wird als aktiver
   Provider verwendet", bzw. ohne diesen Zusatz, wenn schon einer aktiv ist).
3. Liste neu laden, `onProvidersChanged()`.
Schlägt Schritt 1 fehl: Fehler wie bei jedem anderen Hinzufügen, nichts
aktiv gesetzt. Kein Speichern ohne Klick.

**B5 — Ollama ohne Key im Formular.** Für `providerType === "ollama"`:
- API-Key-Feld nicht `required`, Label-Zusatz „(bei Ollama nicht nötig)".
- „Zugangsdaten testen" ist ohne Key freigegeben.
- Hinzufügen/Testen/Modelle laden senden bei leerem Key
  `OLLAMA_PLACEHOLDER_API_KEY` (eine Hilfsfunktion `effectiveApiKey(form)`,
  eine Konstante im Frontend, Wert `"ollama-no-key"`).
- Wählt der Nutzer den Typ `ollama` und ist die Base-URL leer, wird sie mit
  `http://127.0.0.1:11434/v1` vorbelegt (sichtbar, änderbar).
Backend unverändert: für das Backend ist der Platzhalter ein normaler Key.

### 3.C Teil C — Einstieg (BL-0082, BL-0110)

**C1 — Leerer Zustand (BL-0082).** Definition: **„leer" = keine echten
Server**, d. h. `servers.filter(s => !s.isLocal).length === 0`. Der lokale
Pseudo-Server (Spec 0032) zählt nie mit; Gruppen zählen nicht (eine Gruppe
ohne Server ist kein Einstieg in den Pfad).

Ist die Liste leer, zeigt `ServerList` unterhalb des Localhost-Eintrags
einen **Einstiegs-Block** statt des heutigen Entwicklertexts:
- Überschrift „Noch kein Server angelegt", ein Satz Erklärung,
- Primär-Button **„Ersten Server anlegen"** → wechselt zum Tab „Verwalten"
  mit geöffnetem Formular für einen neuen Server
  (`initialSelection = { kind: "newServer" }`, s. Teil 0.7),
- Hinweiszeile: „Ohne Server ausprobieren: ‚Localhost' oben öffnet eine
  Sitzung auf diesem Rechner — mit denselben Bestätigungen wie auf einem
  Server."
Existieren Gruppen, wird der Gruppenbaum zusätzlich darunter wie heute
angezeigt. Sobald ein echter Server existiert, verschwindet der Block.

Zusätzlich in `ServerList.tsx`: „Lade Server…" und „Verbinde…" über
Locale-Keys (beide heute fest deutsch, liegen im selben Bildschirm).

**C2 — Ed25519-Empfehlung (BL-0110), abtrennbar.** Im Server-Formular,
Anmeldeart „Private Key", unter dem Schlüsselfeld: statischer Hinweis
„Empfohlen: Ed25519-Schlüssel. Neu erzeugen mit `ssh-keygen -t ed25519`.
RSA-Schlüssel funktionieren weiterhin." Nur Text, keine Prüfung des
Inhalts, keine Sperre, kein Einfluss auf Speichern/Verbinden.

---

## 4. Design

### 4.1 Fehler-Register des Pfads (Texte DE / EN)

Texte sind Vorschläge; der Coder darf glätten, aber Ursache und nächster
Schritt müssen erhalten bleiben. Kein `{{…}}`-Platzhalter in `errors.*`
(`translateErrorCode` reicht keine Optionen durch).

| Code | DE | EN |
|---|---|---|
| `AI_AUTH_FAILED` (bleibt) | Authentifizierung beim KI-Provider fehlgeschlagen – API-Key in den Einstellungen prüfen. | AI provider rejected the credentials – check the API key in Settings. |
| `AI_MODEL_NOT_FOUND` (neu) | Der KI-Provider kennt dieses Modell nicht. Modellnamen in den Einstellungen prüfen („Modelle laden" zeigt die verfügbaren); bei Ollama das Modell zuerst mit `ollama pull <name>` laden. | The AI provider doesn't know this model. Check the model name in Settings ("Load models" lists the available ones); for Ollama, pull it first with `ollama pull <name>`. |
| `AI_LOCAL_PROVIDER_UNREACHABLE` (neu) | Der lokale KI-Dienst antwortet nicht. Ollama (bzw. deinen lokalen Server) starten und erneut versuchen. | The local AI service isn't responding. Start Ollama (or your local server) and try again. |
| `AI_NETWORK_ERROR` (Text neu) | Keine Verbindung zum KI-Provider. Internetverbindung und – bei eigenem Endpunkt – die Base-URL prüfen, dann erneut senden. | Can't reach the AI provider. Check your internet connection and, for a custom endpoint, the base URL, then send again. |
| `AI_TIMEOUT` (neu) | Der KI-Provider hat zu lange nicht geantwortet. Kurz warten und erneut senden; bei einem lokalen Modell: ist der Rechner ausgelastet oder das Modell zu groß? | The AI provider took too long to respond. Wait a moment and send again; with a local model, check whether the machine is busy or the model too large. |
| `AI_PROVIDER_UNAVAILABLE` (Text neu) | Der KI-Provider meldet einen Fehler (z. B. Wartung oder Überlastung). Später erneut versuchen; hält es an, Provider-Einstellungen prüfen. | The AI provider reported an error (e.g. maintenance or overload). Try again later; if it persists, check the provider settings. |
| `AI_RATE_LIMITED` (bleibt) | unverändert (Spec 0051) | unchanged |
| `AI_NO_ACTIVE_PROVIDER` (neu) | Noch kein aktiver KI-Provider. In den Einstellungen unter „KI-Provider" einen Provider hinzufügen und aktiv setzen. | No active AI provider yet. Add one under Settings → AI provider and set it active. |
| `SSH_CONNECTION_REFUSED` (neu) | Der Server lehnt die Verbindung ab. Port prüfen und ob dort ein SSH-Dienst läuft. | The server refused the connection. Check the port and whether an SSH service is running there. |
| `SSH_HOST_NOT_FOUND` (neu) | Der Hostname ist unbekannt. Schreibweise der Adresse prüfen (oder die IP-Adresse eintragen). | Unknown host name. Check the address spelling (or enter the IP address). |
| `SSH_HOST_UNREACHABLE` (neu) | Der Server ist aus diesem Netz nicht erreichbar. Netzwerk/VPN prüfen. | The server can't be reached from this network. Check your network/VPN. |
| `SSH_TIMEOUT` (Text neu) | Der Server antwortet nicht. Adresse, Port und Firewall prüfen und erneut verbinden. | The server isn't responding. Check address, port and firewall, then connect again. |
| `SSH_CONNECTION_CLOSED` (neu) | Der Server hat die Verbindung während des Aufbaus beendet. Prüfen, ob auf diesem Port ein SSH-Dienst läuft; ggf. kurz warten (Schutz vor zu vielen Versuchen). | The server closed the connection during setup. Check that an SSH service runs on this port; wait a moment if too many attempts were made. |
| `SSH_CONNECTION_FAILED` (bleibt) | unverändert (0047 D2) | unchanged |
| `SSH_AUTH_FAILED` (Text neu) | Anmeldung abgelehnt. Benutzername und Passwort bzw. Schlüssel in den Server-Einstellungen prüfen. | Login rejected. Check the user name and password or key in the server settings. |
| `SSH_HOST_KEY_NOT_TRUSTED` (neu) | Verbindung abgebrochen, weil der Host-Key nicht bestätigt wurde. Wenn du dem Server vertraust: erneut verbinden und den Fingerprint prüfen. | Connection cancelled because the host key wasn't confirmed. If you trust the server, connect again and verify the fingerprint. |
| `SSH_HOST_KEY_CONFIRM_TIMEOUT` (neu) | Die Host-Key-Abfrage wurde nicht rechtzeitig beantwortet; der Key wurde nicht übernommen. Erneut verbinden. | The host key prompt wasn't answered in time; the key was not trusted. Connect again. |

Zusätzlich (keine `errors.*`-Codes, aber im Pfad):
`aiProvider.testResultAuthFailed` → „✗ Authentifizierung fehlgeschlagen –
API-Key prüfen" / „✗ Authentication failed – check the API key";
`aiProvider.testResultRateLimited` → „Zugangsdaten vermutlich gültig, der
Anbieter drosselt aber gerade. In einer Minute erneut testen." / „Credentials
look valid, but the provider is rate-limiting right now. Test again in a
minute."; `serverForm.testResult.authFailed` und `.timeout` analog zu
`SSH_AUTH_FAILED` / `SSH_TIMEOUT`.

Die Host-Key-**Dialoge** selbst (`hostKeyDialog.*`) bleiben unverändert —
sie nennen bereits Ursache und Entscheidung (Fingerprint prüfen, MITM-Hinweis).

`FIVE_MINUTE_PATH_ERROR_CODES` = alle Codes dieser Tabelle.

### 4.2 Unterscheidung der Verbindungsfehler

Der Auftrag verlangt, Verbindungsabbruch, Timeout, „Ollama nicht gestartet"
und „Host nicht erreichbar" auseinanderzuhalten. Festlegung:

**KI-Seite** (Entscheidung am Transportfehler, nicht am Provider-Typ):

| Situation | Erkennung | Code |
|---|---|---|
| Ollama/lokaler Dienst nicht gestartet | `is_connect()` + Loopback-Host | `AI_LOCAL_PROVIDER_UNREACHABLE` |
| Entfernter Provider nicht erreichbar (DNS, abgelehnt, TLS) | `is_connect()`, kein Loopback | `AI_NETWORK_ERROR` |
| Keine Antwort in der Frist | eigener Timeout (`SSE_INACTIVITY_TIMEOUT`) oder `is_timeout()` | `AI_TIMEOUT` |
| Verbindungsabbruch mitten im Stream | Body-/Stream-Fehler, nicht `is_connect()` | `AI_NETWORK_ERROR` |

„Lokal" heißt Loopback-Adresse, nicht Provider-Typ `ollama`: dieselbe
Meldung passt für LM Studio o. Ä. hinter einem generischen Endpunkt auf
`localhost`, und die Entscheidung braucht keinen Provider-Kontext im
Fehler-Mapping (`err.url()` genügt). Verworfen: Unterscheidung nach
Provider-Typ in `app-shell` — bräuchte den Typ an jeder Fehlerstelle und
würfe Ollama auf einem anderen Rechner fälschlich in „starte Ollama".

**SSH-Seite** (erster Hop):

| Situation | Erkennung | Code |
|---|---|---|
| Name nicht auflösbar | `ConnectionFailed` + nachträgliches `lookup_host` scheitert | `SSH_HOST_NOT_FOUND` |
| Port zu / kein Dienst | `io::ErrorKind::ConnectionRefused` | `SSH_CONNECTION_REFUSED` |
| keine Route | `HostUnreachable`/`NetworkUnreachable` | `SSH_HOST_UNREACHABLE` |
| keine Antwort | `SSH_CONNECT_TIMEOUT` abgelaufen oder `TimedOut` | `SSH_TIMEOUT` |
| Abbruch während Aufbau | `Disconnect`, `ConnectionReset`/`Aborted`/`UnexpectedEof` | `SSH_CONNECTION_CLOSED` |
| sonst | — | `SSH_CONNECTION_FAILED` |

DNS wird **nachträglich** diagnostiziert statt vorab aufgelöst: so bleibt
der erfolgreiche Verbindungsaufbau und der Host-Key-Abgleich (der am
Hostnamen hängt) unverändert. Verworfen: Vorab-Auflösung und Übergabe der
IP an `russh` — spart eine DNS-Anfrage im Fehlerfall, ändert aber den
sicherheitsrelevanten Pfad.

### 4.3 DTO-Änderungen (alle additiv, E3)

- `CommandError.code`: neue Werte, Form unverändert.
- `TestConnectionResult::NetworkError { message, code }` — neues optionales
  Feld.
- `TestAiProviderCredentialsResult::Unreachable { message, code }` — neues
  optionales Feld.
- Kein neuer Tauri-Command. Keine Änderung an DB-Schema, Sync-Format,
  Lizenzformat.
Ein altes Frontend ignoriert das neue Feld; ein unbekannter Code fällt auf
`message` zurück.

### 4.4 Provider-Unterschiede

- **Anthropic:** Modell-Fehler per Body-Marker (Teil 0.2); Discovery gibt es
  für Anthropic nicht (unverändert), also auch keine Ollama-Probe-Pfade.
- **OpenAI / OpenRouter / generisch:** wie oben; OpenRouter meldet ein
  unbekanntes Modell ggf. mit 400 (zu verifizieren) — deshalb 400 **nur mit**
  Modell-Marker.
- **Ollama:** Platzhalter-Key; lokal → `AI_LOCAL_PROVIDER_UNREACHABLE`.
  Ein Ollama auf einem anderen Rechner verhält sich wie ein entfernter
  generischer Endpunkt.

### 4.5 Wechselwirkungen mit Querschnitts-Mechanismen

| Mechanismus | Wirkung |
|---|---|
| Redaction | Keine neue Datensenke. Die Probe nutzt den bestehenden Log-Pfad von `discover_models` (redigiert über `secrets`). Der Platzhalter `ollama-no-key` ist kein Geheimnis; er wird wie jeder Key aus Logs entfernt — deshalb ein Wert, der nicht in normalem Text vorkommt (nicht „ollama"). |
| Fencing, Kompaktierung, Prompt-Caching, `max_tokens` | nicht berührt (kein neuer KI-Aufruf, keine Prompt-Änderung). |
| Rate-Limit-Gate | Die Probe läuft über `discover_models`, das heute schon am Gate vorbeiläuft (Discovery ist kein Chat-Aufruf) — unverändert. Retry nur bei 429, neue Varianten werden nie wiederholt. |
| Stopp/Einreihen | nicht berührt. |
| Ledger/Audit | nicht berührt (keine Aktion auf einem Server). |
| MCP | `connect_session`-Timeout gilt auch für MCP (§3.A3); Fehlertext an den MCP-Client ist die `message` wie bisher. |
| Nie hängen | neuer Timeout im SSH-Connect; `lookup_host` in der Diagnose läuft innerhalb dieses Timeouts. |
| BL-0042 | Probe nur auf Nutzeraktion (E1), nur Loopback, nie beim Start. |

---

## 5. Sicherheits-Invarianten

- **Host-Keys (Invariante 5):** Kein Pfad dieser Spec akzeptiert einen
  Host-Key. Der Connect-Timeout liefert immer einen Fehler, nie
  `Connected`, nie `trust()`. Er umschließt nicht das Warten auf die
  Host-Key-Entscheidung. `resolve_or_pending`, `ClientHandler` und der
  Hostname, unter dem Host-Keys gespeichert/geprüft werden, bleiben
  unverändert.
- **Fehlerpfade erscheinen im UI:** Jede neue Variante wird als Fehler mit
  Code angezeigt; kein neuer Pfad bricht still ab. Unbekannter Code →
  Rohtext (nie leere Anzeige).
- **Keine Secrets in Meldungen:** Die neuen `SshError`-Payloads enthalten
  nur `io::Error`-Text bzw. Hostname; die `AiError`-Payloads denselben
  Inhalt wie bisher `ProviderUnavailable`/`NetworkError`. Das Frontend
  zeigt bei bekanntem Code den Payload gar nicht (ADR 0039).
- **Credential-Handling:** `CredentialStore` unverändert. Der
  Ollama-Platzhalter geht über den bestehenden `add_ai_provider`-Pfad.
- **Filter-Engine, Risiko, Confirm/AutoExec, Redactor:** nicht berührt.
- **Kein Phone-Home:** E1; Probe-Ziel fest `127.0.0.1:11434`.
- **Keine stillen Rückfälle:** Ein Vorschlag wird nie ohne Klick übernommen
  (E2); ein fehlgeschlagenes Anlegen setzt nichts aktiv.

---

## 6. Tests

Jeder Regressionstest muss gegen den ungefixten Stand scheitern (CLAUDE.md);
im Bericht bestätigen.

**Teil A — Rust**
1. `map_http_status`: 404 mit Ollama-, OpenAI-, Anthropic-Fixture →
   `ModelNotFound`; 400 mit OpenRouter-Fixture → `ModelNotFound`.
   *Scheitert heute:* liefert `ProviderUnavailable`.
2. Negativ: 404 ohne Modell-Marker (z. B. falsche Base-URL, HTML-Body) →
   `ProviderUnavailable`; 400 ohne Marker → `ProviderUnavailable`; 401 mit
   „model" im Body → weiter `AuthenticationFailed`.
3. `map_transport_error` gegen `127.0.0.1:<freier Port>` →
   `LocalProviderUnreachable`; gegen einen nicht auflösbaren Namen unter
   `.invalid` → `NetworkError`. *Scheitert heute:* beides `NetworkError`.
4. Discovery-/Chat-Timeout (bestehende Tests mit kurzem Timeout) → jetzt
   `AiError::Timeout`, `Display`-Text wörtlich wie vorher.
5. `classify_credential_test_result` mit `RateLimited`, `ModelNotFound`,
   `LocalProviderUnreachable` → `Unreachable { code: Some(…) }` mit dem
   jeweiligen Code (BL-0153). *Scheitert heute:* kein `code`-Feld.
6. `map_russh_error` für jede `io::ErrorKind` der Tabelle §4.2 und für
   `Disconnect` → erwartete Variante; unbekannte Kind → `ConnectionFailed`.
7. `ssh-transport` Integrationstest: Verbindung zu einem geschlossenen
   lokalen Port → `ConnectionRefused`; zu `name.invalid` → `HostNotFound`.
8. `test_connection`: `NetworkError` trägt `code`.
9. `active_ai_provider_config` ohne aktiven Provider → Code
   `AI_NO_ACTIVE_PROVIDER`.
10. Eindeutigkeitstests für alle drei Code-Familien um die neuen Werte
    erweitert.

**Teil A3 — adversarial (ERHÖHT)**
11. **Timeout gewährt nie:** Hilfsfunktion `connect_with_timeout(fut,
    timeout)` mit einer nie fertig werdenden Future → `Err(SSH_TIMEOUT)`;
    ein `HostKeyStore`-Mock zählt `trust()`-Aufrufe → 0.
12. **Timeout umfasst nicht die Host-Key-Wartezeit:** Future liefert sofort
    `PendingHostKeyConfirmation`; danach vergeht mehr als
    `SSH_CONNECT_TIMEOUT`, bevor die (simulierte) Entscheidung kommt → kein
    `SSH_TIMEOUT`; es gilt ausschließlich der Host-Key-Timeout.
13. **Hängender Handshake:** lokaler `TcpListener`, der annimmt, aber nie ein
    SSH-Banner schickt → `connect_with_timeout(ssh_transport::connect(…))`
    endet nach dem (für den Test verkürzten) Timeout mit `SSH_TIMEOUT`.
    Test selbst mit äußerem Timeout, damit er nie hängt.
14. **Host-Key-Pfad unverändert:** Integrationstest gegen den Test-Fixture-
    Server: unbekannter Key → weiterhin `PendingHostKeyConfirmation`
    (nicht als `ConnectionClosed`/`ConnectionFailed` fehlklassifiziert);
    bekannter Key unter dem Hostnamen → `Connected`.
15. **Mismatch bleibt Mismatch:** geänderter Key → weiterhin
    `PendingHostKeyConfirmation { Mismatch }`, nie ein Verbindungsfehler-Code.
16. **DNS-Diagnose nur im Fehlerfall:** erfolgreicher Connect ruft
    `lookup_host` nicht zusätzlich auf (Zähler über eine injizierbare
    Lookup-Funktion oder Nachweis per Code-Struktur im Review).
17. **Keine Secrets im Fehler:** Connect mit Passwort gegen geschlossenen
    Port → weder `message` noch Log enthalten das Passwort.

**Teil A — Frontend (Vitest)**
18. Für jeden Code in `FIVE_MINUTE_PATH_ERROR_CODES`: in `KNOWN_ERROR_CODES`,
    DE- und EN-Text vorhanden und nicht leer, DE ≠ EN. *Scheitert heute:*
    Liste und neue Codes fehlen.
19. Testen-Button: `unreachable` mit `AI_RATE_LIMITED` zeigt
    `testResultRateLimited`; mit `AI_MODEL_NOT_FOUND` den übersetzten Text;
    ohne Code den bisherigen Text mit `message`.
20. `TestResultBadge`: `networkError` mit `SSH_CONNECTION_REFUSED` → übersetzt;
    ohne Code → bisheriger Text.

**Teil B**
21. Probe-Auslöser: Mount ohne Ollama-Provider → genau ein
    `discoverModels`-Aufruf mit `127.0.0.1:11434/v1`; Mount mit
    vorhandenem Ollama-Provider → kein Aufruf; „Erneut suchen" → ein
    weiterer Aufruf. (Mock von `api.ts`.)
22. Kein Aufruf ohne Mount: `App`-Start ohne geöffnete Einstellungen löst
    keinen `discoverModels`-Aufruf aus.
23. Ergebnisse: Modelle → Vorschlagskarte; leer → Pull-Anleitung;
    `AI_LOCAL_PROVIDER_UNREACHABLE` → Installationsanleitung (nur ohne
    Provider oder bei Typ `ollama`); anderer Fehler → keine Karte.
24. „Ollama übernehmen" ohne aktiven Provider → `addAiProvider` mit
    erwarteten Feldern, danach `setActiveAiProvider`; mit aktivem Provider →
    kein `setActiveAiProvider`; `addAiProvider` schlägt fehl → kein
    `setActiveAiProvider`.
25. Ohne Klick wird nie `addAiProvider` aufgerufen (auch nicht nach
    erfolgreicher Probe).
26. `effectiveApiKey`: Ollama + leer → Platzhalter; Ollama + Wert → Wert;
    anderer Typ + leer → leer (kein Platzhalter für Cloud-Provider).

**Teil C**
27. `ServerList`: nur Localhost → Einstiegs-Block sichtbar, Entwicklertext
    nicht mehr vorhanden; ein echter Server → kein Block; nur Gruppen, kein
    Server → Block + Gruppenbaum.
28. Klick „Ersten Server anlegen" → Callback mit `{ kind: "newServer" }`.
29. Server-Formular, Anmeldeart „Private Key" → Ed25519-Hinweis sichtbar;
    andere Anmeldearten → nicht sichtbar.

**Manuelle Testabläufe (für Stefan, am echten Gerät)** — der Coder liefert
sie ausformuliert mit:
- Falscher Anthropic-/OpenAI-Key: Testen-Button und Chat.
- Falscher Modellname bei Anthropic, OpenAI, OpenRouter, Ollama.
- Ollama beendet → Chat-Nachricht; Ollama-Einstellungen öffnen (Anleitung);
  Ollama starten → „Erneut suchen" → Vorschlag → übernehmen → verbinden.
- Ollama läuft ohne Modell → Pull-Anleitung.
- Server mit Tippfehler im Namen, geschlossenem Port, nicht routbarer
  Adresse (z. B. eine ungenutzte private IP) → je eigene Meldung; letzte
  nach etwa 10 s statt nach Minuten.
- Host-Key ablehnen; Host-Key-Dialog offen lassen bis zum Timeout.
- Frische Installation (leeres Datenverzeichnis): Einstiegs-Block → Server
  anlegen.
- Alles einmal in englischer UI.

---

## 7. Umsetzungsreihenfolge

Jeder Schritt ein eigener Commit, Gate grün.

0. Teil-0-Bericht (0.1–0.7), vor jedem Code.
1. **A3** — `SshError`-Varianten, `map_russh_error`, DNS-Diagnose,
   gemeinsame Timeout-Konstante, `connect_with_timeout` in `connect_session`,
   Tests 6, 7, 11–17. *(ERHÖHT, sicherheitsnah zuerst.)*
2. **A2** — `AiError`-Varianten, `map_http_status`, `map_transport_error`,
   Timeout-Konstruktor, Tests 1–4.
3. **A4** — Codes in `app-shell` (DTO-Felder, `discover_models`,
   Host-Key-Zweige, kein aktiver Provider), Tests 5, 8–10.
4. **A5** — Frontend-Register, Locales, Testen-Button, Badge, Tests 18–20.
5. **B** — Probe, Karte, Übernehmen, Ollama ohne Key, Tests 21–26.
6. **C1** — Einstiegs-Block, Navigation, Locale-Keys in `ServerList`,
   Tests 27–28.
7. **C2** — Ed25519-Hinweis, Test 29. Unabhängig; darf auch einzeln
   vorgezogen oder abgetrennt werden.

Konfliktträchtige Dateien: `commands.rs` (Schritte 1, 3), Locale-Dateien
(4–7), `AiProviderSettings.tsx` (4, 5). Parallel laufende Aufträge in
diesen Dateien vorher abstimmen.

Abschluss: `spec-reviewer` NORMAL, für Schritt 1 ERHÖHT mit den
Angriffsrichtungen „Kann ein Timeout oder eine Fehlklassifizierung einen
Host-Key akzeptieren, eine Host-Key-Abfrage überspringen oder abbrechen,
bevor der Nutzer entscheidet?" und „Weicht der Erfolgs-/Host-Key-Pfad in
`connect()` vom bisherigen Verhalten ab?". CHANGELOG (Added: Ollama-
Erkennung, Einstieg; Changed: verständlichere Fehlermeldungen, SSH-
Verbindungsaufbau bricht nach 10 s ohne Antwort ab). ADR für die
Loopback-Regel und die nachträgliche DNS-Diagnose.

---

## 8. Offene Punkte

Beide Punkte am 2026-09-22 von Stefan entschieden, siehe Klarstellungen (§ 9).

1. **Reichweite von E3 — `code`-Feld in zwei Ergebnis-DTOs (K3,
   Schnittstelle).** `TestConnectionResult::NetworkError` und
   `TestAiProviderCredentialsResult::Unreachable` sind Fehlerausgänge,
   formal aber Ergebnis-DTOs, nicht `CommandError`.
   - Option a: gedeckt durch E3 (additiv, optional) — so beschrieben.
   - Option b: nicht gedeckt; dann zeigen Testen-Button und
     „Verbindung testen" im Fehlerfall weiter Rohtext, und BL-0153 bleibt
     offen.
   **Empfehlung: a.** Blockiert nur Schritt 3/4 teilweise; A2/A3 laufen
   unabhängig.
2. **Erster manuell angelegter Provider automatisch aktiv? (K3, Verhalten
   eines bestehenden Ablaufs).** Heute ist jeder neue Provider inaktiv; auf
   dem Pfad muss man nach „Hinzufügen" noch „Aktiv setzen". Diese Spec
   aktiviert nur beim **Ollama-Vorschlag** und nur nach Klick (B4).
   - Option a: auch im normalen Formular — ist noch kein Provider aktiv,
     wird der neue aktiv (Hinweis im Formular).
   - Option b: so lassen; Einstieg und Banner verweisen auf „Aktiv setzen".
   **Empfehlung: a**, als eigenes kleines Item nach dieser Spec (nicht
   blockierend, nicht Teil dieser Umsetzung).

---

## 9. Klarstellungen

(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)

- 2026-09-22 · Offener Punkt 1 (Tor 1) · **Option a:** E3 deckt das optionale
  `code`-Feld in `TestConnectionResult::NetworkError` und
  `TestAiProviderCredentialsResult::Unreachable` ab (additiv, optional).
- 2026-09-22 · Offener Punkt 2 (Tor 1) · **Option a, aber nicht in dieser
  Spec:** Ein manuell angelegter erster Provider soll automatisch aktiv werden;
  das läuft als eigenes Item. Diese Spec aktiviert weiterhin nur beim
  Ollama-Vorschlag nach Klick (B4).
