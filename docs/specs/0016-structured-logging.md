# Spec: Strukturiertes Logging & Diagnose

Status: Entwurf
Modul: Erweiterung über alle Crates hinweg (`ssh-manager-core`,
`ai-providers`, `ssh-transport`, `app-tauri`)
Abhängigkeiten: keine fachliche, aber betrifft praktisch jede bisherige Spec
punktuell (Logging-Aufrufe an den relevanten Stellen)

## 1. Ziel

Der komplette Pfad eines KI-Vorschlags — gesendeter Kontext, empfangene
Rohantwort, Tool-Call-Parsing, Validierung, Filter-Engine-Entscheidung,
Ausführung — wird strukturiert geloggt, damit Fehler wie der beobachtete
`target_id ist keine gültige UUID`-Fall sofort und ohne Rätselraten
nachvollziehbar sind. Die Logs müssen sowohl für dich manuell einsehbar sein
als auch **direkt von einer Claude-Code-Instanz lesbar**, ohne Umweg über
eine App-UI (einfacher Dateipfad reicht).

## 2. Technologie

**`tracing`** + `tracing-subscriber` (mit `tracing-appender` für
Datei-Rotation). Begründung: De-facto-Standard im Rust-Ökosystem für
strukturiertes, spanbasiertes Logging — erlaubt, zusammengehörige
Log-Zeilen über ein `session_id`/`action_id`-Span zu korrelieren, statt nur
unzusammenhängende Text-Zeilen zu haben.

## 3. Speicherort und Format

Plattformspezifischer Log-Ordner über die `directories`-Crate:

- macOS: `~/Library/Logs/Smart SSH/`
- Windows: `%APPDATA%\Smart SSH\logs\`
- Linux: `~/.local/state/smart-ssh/logs/`

Format: **JSON Lines** (eine Zeile pro Log-Event, maschinenlesbar) —
bewusst kein reiner Klartext-Log, damit sowohl du als auch eine
Claude-Code-Instanz gezielt filtern/greppen können (z. B. nach
`"level":"ERROR"` oder einer bestimmten `session_id`). Tägliche Rotation,
Aufbewahrung der letzten 14 Tage, ältere Dateien werden beim Start
automatisch gelöscht.

## 4. Was geloggt wird

Pro KI-Anfrage-Zyklus (ein `send_chat_message`-Aufruf), verknüpft über ein
gemeinsames `request_id`-Span-Feld:

1. **Ausgehender Kontext**: `SessionContext`, der tatsächlich an den
   Provider geht — **nach** Redaction (Spec 0006, Abschnitt 5), nie davor.
   Wichtig: Logs sind kein Schlupfloch für Secrets, die die Redaction
   eigentlich unterdrücken soll — dieselbe Redaction-Regel gilt für Logs wie
   für den tatsächlichen API-Request.
2. **Empfangene Rohantwort** je Streaming-Chunk (kompakt, z. B. Tool-Call-
   JSON-Fragmente vollständig, reine Text-Deltas ggf. zusammengefasst statt
   Zeichen für Zeichen).
3. **Tool-Call-Parsing/Validierung**: bei Erfolg das geparste `AiAction`,
   bei Fehler die **vollständige Rohantwort des Providers** plus die genaue
   Fehlermeldung (Feld, erwarteter vs. tatsächlicher Typ) — genau das, was
   im beobachteten Bugfall gefehlt hätte, um sofort zu sehen, was die KI
   tatsächlich als `target_id` geschickt hat.
4. **Filter-Engine-Entscheidung**: Kommando, `Decision`, welche Regel/
   Hard-Blacklist-Eintrag gegriffen hat (wiederverwendet dieselbe
   `EvaluationTrace`-Struktur aus Spec 0009, Abschnitt 4).
5. **SSH-Ausführung**: Kommando, Exit-Code, Redacted-Output-Länge (nicht
   zwingend der volle Output bei sehr langen Kommandos, um die Logs nicht
   unnötig aufzublähen — Kürzung mit Hinweis "gekürzt, voller Output nicht
   geloggt" ab einer konfigurierbaren Länge).
6. **Session-Lifecycle**: Connect/Disconnect, Host-Key-Ereignisse — jeweils
   mit Grund/Status.

## 5. Zugriff

- **Für dich**: neuer Command `open_log_directory()`, öffnet den
  Log-Ordner im System-Dateimanager (Finder/Explorer) über das Tauri-
  Dialog-/Opener-Plugin — ein Klick in den Einstellungen reicht, kein
  manuelles Navigieren zum plattformspezifischen Pfad nötig.
- **Für Claude Code**: keine neue Schnittstelle nötig — der Pfad aus
  Abschnitt 3 ist fix und dokumentiert, eine Claude-Code-Instanz mit
  Terminal-Zugriff kann die JSON-Lines-Dateien direkt lesen/greppen
  (`tail -f`, `jq`, etc.), ganz ohne App-Interaktion.

## 6. Konkreter Bugfix: `target_id` bei `ProposeNoteUpdate`

Der beobachtete Fehler ist Anlass für eine Design-Korrektur, nicht nur ein
Logging-Thema: Die KI sollte für "die Notiz des aktuell verbundenen Servers"
**keine eigene ID raten/erfinden müssen**. Anpassung an
`AiAction::ProposeNoteUpdate` (Spec 0003, Abschnitt 5.2) bzw. dessen
Tool-Schema (Spec 0006, Abschnitt 3):

- Das der KI angebotene Tool-Schema für `ProposeNoteUpdate` bekommt **kein**
  Freitext-`target_id`-Feld mehr für den Regelfall. Stattdessen: ein
  optionales Enum-Feld `target: "current_server" | "current_server_group"`
  (Default: `current_server`, falls das Feld fehlt). Das Backend löst daraus
  die tatsächliche `ServerId`/`GroupId` **selbst** aus dem Session-Kontext
  auf — die KI muss nie eine ID kennen oder korrekt formatieren.
- Zusätzlich: **Fehler-Containment.** Ein Fehler beim Parsen/Validieren
  eines Tool-Calls darf **niemals** die SSH-Verbindung/Session beenden,
  sondern nur als sichtbarer Fehler-Hinweis im Chat erscheinen (wie im
  Screenshot bereits der Fall) — die Session bleibt aktiv nutzbar. Sollte
  aktuell doch die Verbindung mitgerissen werden, ist das ein separater
  Bug im Error-Handling, den es zu identifizieren gilt (siehe Abschnitt 7).

## 7. Offene Punkte / zu untersuchen

- Der beobachtete Absturz der gesamten Verbindung (nicht nur ein
  Fehler-Hinweis) deutet auf fehlendes Error-Containment an der
  Tool-Call-Verarbeitungsstelle hin — mit dem neuen Logging aus dieser Spec
  sollte sich die genaue Stelle beim nächsten Auftreten sofort
  identifizieren lassen, statt weiter zu raten.
- Die gemeldete Unmöglichkeit, Text im Chat/Terminal-Bereich zu markieren,
  ist aktuell nicht erklärt — möglicherweise blockiert das Fehler-Overlay
  Zeigegeräte-Ereignisse für den darunterliegenden Bereich. Braucht eigene
  Untersuchung, ggf. losgelöst vom KI-Provider-Bug.
