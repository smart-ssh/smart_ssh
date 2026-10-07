# ADR 0109 — Schritt-Protokoll für Verbindungstest und Verbinden

Status: akzeptiert
Betrifft: Spec 0005, Spec 0008 (Abschnitt 7), Spec 0069 (Teil A3), Spec 0076,
Spec 0094, Spec 0063, ADR 0008, Issue #51

## Problem

Ein gescheiterter (oder langsamer) Verbindungstest bzw. Verbindungsaufbau
zeigte nur eine Zeile. Woran es lag — DNS, Port, Handshake, Host-Key,
Anmeldung, welcher Hop — blieb offen. Issue #51 verlangt ein zugeklapptes,
strukturiertes Protokoll je Versuch und Hop, nur in der Oberfläche.

## Entscheidung

1. **Typen in `core`** (`ssh::connect_log`): `ConnectStep` (Enum mit festen,
   ungefährlichen Parametern), `StepStatus` (`running`/`ok`/`failed{code}`),
   `ConnectStepRecord` (Hop-Index, `user@host:port`, Schritt, Status, Dauer)
   und `ConnectLog`, ein geteilter Speicher (`Arc<Mutex<…>>`). Ein Fehlschlag
   trägt nur einen stabilen Code (`SshError::code()` oder `HOST_KEY_UNKNOWN`/
   `HOST_KEY_CHANGED`), nie freien Bibliothekstext. Für Anmeldemethoden gibt
   es nur die Art (`AuthMethodKind`) — für Geheimnisse ist strukturell kein
   Feld da.
2. **`ssh_transport::connect_with_log`** zeichnet auf; `connect` ist ein
   Wrapper mit verworfenem Protokoll (MCP und alle übrigen Aufrufer
   unverändert). Ein Fehler schließt den laufenden Schritt mit seinem Code.
   Bricht ein äußerer Timeout den Versuch ab, schließt der Aufrufer den
   laufenden Schritt mit `SSH_TIMEOUT` (`fail_running`) — deshalb ist das
   Protokoll geteilt und nicht Rückgabewert.
3. **Erster Hop: DNS und TCP als eigene Schritte.** Statt
   `russh::client::connect((host, port))` löst `connect` selbst auf
   (`tokio::net::lookup_host`), verbindet per `TcpStream::connect` auf die
   aufgelösten Adressen, setzt `nodelay` und ruft `connect_stream`. Das ist
   genau der Ablauf, den `russh::client::connect` intern hat; ausgeschrieben
   nur, damit beide Schritte getrennt messbar sind. Der Hostname bleibt der
   Schlüssel der Host-Key-Prüfung. Ein Auflösungsfehler wird direkt zu
   `HostNotFound` (gleicher Text wie die bisherige Nachdiagnose); TCP-Fehler
   laufen durch dieselbe `io::ErrorKind`-Tabelle, die Nachdiagnose aus Spec
   0069 A3 bleibt für `ConnectionFailed` bestehen. Das ersetzt die Aussage
   „nie eine Vorab-Auflösung" aus Spec 0069 A3; am Ergebnis eines Versuchs
   ändert es nichts.
4. **Hops ab dem zweiten: `tunnelOpen` statt DNS/TCP.** Dort löst der
   Jump-Host auf und verbindet, nicht dieser Rechner; aufgezeichnet wird das
   Öffnen des `direct-tcpip`-Kanals.
5. **Handshake und Host-Key im Handler.** `kex_done` (läuft in russh 0.63
   vor `check_server_key`) liefert Versionskennung und ausgehandelte
   Algorithmen (kex, Host-Key, Cipher, Client-MAC); nur beim ersten
   Schlüsseltausch. Das gemeinsame Geheimnis wird nicht angefasst. Die
   Versionskennung kommt vom Server und wird auf druckbares ASCII (max. 255
   Zeichen) reduziert. `check_server_key` zeichnet Typ, SHA256-Fingerprint
   und Ergebnis (`known`/`unknown`/`changed`) auf und gibt das Ergebnis von
   `evaluate_host_key` unverändert an russh zurück.
6. **Anmeldung:** ein Schritt je Hop mit der konfigurierten Methode. Bei
   Ablehnung stehen `remaining_methods`/`partial_success` des Servers dabei —
   nur zur Anzeige, es wird keine weitere Methode versucht.
7. **„Channel/Session open"** ist der `direct-tcpip`-Kanal je Jump-Hop und
   ein abschließender Schritt `sessionReady` am letzten Hop. Der
   Verbindungstest öffnet **keinen** zusätzlichen Session-Kanal: Spec 0008
   Abschnitt 7 prüft bewusst nur den Anmelde-Handshake, und ein normaler
   Verbindungsaufbau öffnet Kanäle erst bei Bedarf.
8. **Transport zum Frontend:** `test_connection` liefert
   `TestConnectionReport` = `TestConnectionResult` flach eingebettet plus
   `steps` (bei Erfolg und Fehlschlag; JSON-Form für alte Leser unverändert).
   Ein gescheitertes `connect`/`resume_chat_session` hängt das Protokoll des
   letzten Versuchs als optionales Feld `connect_log` an den `CommandError`
   (wie `feature_locked`), auch bei abgelehntem Host-Key und abgelaufener
   Host-Key-Abfrage. MCP liest davon nur `message`.
9. **Flüchtig:** kein `tracing`-Aufruf nimmt Schritte oder `connect_log` auf;
   damit stehen sie weder in der Log-Datei noch im Diagnose-Export (der die
   Log-Datei bündelt). Das Frontend hält das Protokoll nur im
   Komponentenzustand.

## Abgewogene Alternativen

- **DNS parallel zum unveränderten `client::connect` messen:** doppelte
  Auflösung, und die angezeigten Adressen wären nicht sicher die
  verwendeten.
- **Protokoll in jede `TestConnectionResult`-Variante:** sieben Varianten
  mit demselben Feld; die flache Einbettung lässt die Form unverändert.
- **Session-Kanal im Test öffnen:** prüft mehr, ändert aber das Ergebnis des
  Tests gegenüber Spec 0008 (Server, die Anmeldung, aber keine Sitzung
  erlauben, würden „fehlgeschlagen").

## Konsequenzen

- Ein zusätzlicher `russh`-Handler-Hook (`kex_done`); bei einem
  russh-Update ist zu prüfen, dass er weiterhin vor `check_server_key`
  läuft (die Integrationstests würden einen fehlenden Handshake-Schritt
  bemerken).
- Tests: `ssh-transport/tests/integration.rs` (`test_issue_51_*`: Erfolg,
  DNS, abgelehnter Port, Timeout, unbekannter und geänderter Host-Key,
  falsches Passwort, Jump-Host je Hop, keine Geheimnisse für jede
  Anmeldeart), `app-logic::test_connection` (Timeout-Markierung, JSON-Form,
  nichts im Log), `app-logic::error` (`connect_log` nur bei Bedarf),
  Frontend `connectStepLog.test.ts`, `ConnectStepLog.test.tsx`,
  `ServerList.test.tsx`, `ServerForm.test.tsx`.
