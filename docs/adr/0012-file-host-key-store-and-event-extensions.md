# 0012-file-host-key-store-and-event-extensions

## Status
Accepted

## Kontext

Beim Verdrahten der Kernschleife aus `docs/specs/0007-tauri-app-mvp.md`
(Teil 2, Session-Handling/Terminal/Chat) mussten mehrere Stellen entschieden
werden, die die Spec entweder offen lässt oder deren Skizze sich beim
Implementieren als unvollständig herausstellte.

**1. `HostKeyStore` brauchte eine echte, persistente Implementierung.**
`AppState` (Spec 0007, Abschnitt 3) verlangt `host_key_store`, aber bislang
existierte im Workspace nur eine In-Memory-Testdouble. Ohne persistente
Speicherung würde eine per Trust-on-First-Use bestätigte Verbindung bei
jedem Neustart erneut den Host-Key-Dialog auslösen. `HostKeyStore` (Spec
0005, Abschnitt 6) ist bewusst **synchron** (`fn check`/`fn trust`, kein
`async_trait`) — `check()` wird aus `russh`s `check_server_key`-Callback
heraus aufgerufen, der selbst innerhalb eines von `russh` gespawnten
Tokio-Tasks läuft. `sqlx` (bereits für alles andere in `persistence-sqlite`
verwendet) ist ausschließlich async; ein synchroner Zugriff darauf hätte
entweder `tokio::task::block_in_place` + `Handle::block_on` gebraucht
(funktioniert nur auf einer Multi-Thread-Runtime und bricht bei jeder
künftigen Änderung der Runtime-Konfiguration lautlos wieder ab) oder einen
fire-and-forget-Hintergrundkanal für Schreibzugriffe erfordert.

**2. Zwei Events aus Spec 0007 Abschnitt 5 reichten nicht aus.** Die
Event-Skizze der Spec deckt einen Fehlerfall während eines Chat-Turns
(`AiEvent::Error`, ein fehlgeschlagenes `SshTransport::execute()`, ein
fehlgeschlagenes `ProfileStore::record_note_revision()`) nicht ab, und
`chat-action-result` kennt laut Skizze nur `{ output: CommandOutput }` —
für `AiAction::ProposeNoteUpdate` (kein `CommandOutput`) fehlt eine
passende Darstellung.

## Entscheidung

**`FileHostKeyStore`** (`crates/app-tauri/src/host_key_store.rs`): eine
JSON-Datei neben der SQLite-Datenbank (`host_keys.json`), synchrones
`std::fs`-I/O, geschützt durch einen `std::sync::Mutex`. `HostKeyStore`s
eigene Trait-Dokumentation nennt eine Datei ausdrücklich als zur
SQLite-Tabelle gleichwertige Option — dieser Weg umgeht das
Sync/Async-Problem vollständig, ohne Blocking-in-Runtime-Fallstricke, und
ist bei der zu erwartenden Schreibfrequenz (ein `trust()`-Aufruf pro neu
gesehenem Host) performant genug. Fingerprints werden als echtes SHA-256
im OpenSSH-üblichen Format `SHA256:<base64>` berechnet (nicht als roher
Hex-Dump des kompletten Public Keys wie in den bisherigen Test-Doubles) —
das ist die Darstellung, die Nutzer von `ssh-keygen -lf`/OpenSSH kennen
und mit einer anderen Quelle abgleichen können. Schreibzugriffe laufen
über eine temporäre Datei + `rename` (atomar), damit ein Absturz
mitten im Schreiben nicht die bestehende, gültige Datei durch eine halb
geschriebene ersetzt.

**Zusätzliches Event `chat-error { session_id, message }`**
(`crates/app-tauri/src/events.rs`): fängt `AiEvent::Error` sowie
fehlgeschlagene Kommando-Ausführungen/Notiz-Updates auf. Ohne dieses Event
würde ein Chat-Turn bei einem Provider-Fehler (Auth fehlgeschlagen,
Rate-Limit, Netzwerkfehler) oder einer fehlgeschlagenen Ausführung ohne
jede sichtbare Erklärung im Frontend abbrechen.

**Erweitertes `chat-action-result`**: `result` ist jetzt ein getaggtes
Enum (`ActionResultPayload::Command{...}` / `::NoteUpdate{summary}`) statt
nur `CommandOutput`, damit auch `ProposeNoteUpdate`-Ergebnisse eine
passende Darstellung bekommen.

## Konsequenzen

**Positiv:**
- Trust-on-First-Use überlebt tatsächlich einen App-Neustart — ohne das
  wäre die Host-Key-Bestätigung aus Spec 0005 Abschnitt 6 in der echten
  App witzlos.
- Kein Risiko von `block_in_place`-Panics bei einer künftigen
  Runtime-Konfigurationsänderung (Tauri wechselt z. B. auf eine
  Single-Thread-Runtime).
- Fehler in einem Chat-Turn sind für den Nutzer sichtbar statt ein
  stilles Verstummen der UI zu verursachen.

**Negativ / Trade-off:**
- Zwei getrennte Persistenzmechanismen für App-Daten (SQLite für
  Server/Gruppen/Provider, eine JSON-Datei für Host-Keys) statt eines
  einzigen — für die aktuelle Datenmenge (wenige bekannte Hosts) unkritisch,
  könnte bei sehr vielen bekannten Hosts (hunderte) ein Argument für einen
  Wechsel auf eine echte Tabelle mit synchronem Treiber (z. B. `rusqlite`
  statt `sqlx`) werden.
- `chat-error`/die erweiterte `chat-action-result`-Form weichen von der
  Spec-Skizze (Abschnitt 5) ab — wer nur die Spec liest, kennt beide nicht.
