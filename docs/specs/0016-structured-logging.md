# Spec 0016 — Strukturiertes Logging und Diagnose

Status: umgesetzt
Zweck: Die App schreibt eine maschinenlesbare Logdatei, mit der sich ein Fehler im Pfad eines KI-Vorschlags (Anfrage, Antwort, Prüfung, Ausführung) nachvollziehen lässt, ohne dass die Logdatei zu einer Senke für Inhalte wird.
Bezüge: Spec 0094 (kein Inhalt auf dem Standard-Level), Spec 0006 (Redaction), Spec 0009 (Filter-Entscheidung), Spec 0063 (Diagnose-Export), ADR 0086.

## 1. Ziel

Der Pfad eines KI-Vorschlags lässt sich aus der Logdatei nachvollziehen,
auch von Werkzeugen mit Terminalzugriff und ohne Umweg über die
Oberfläche.

## 2. Level

- Das Level lässt sich über die Umgebungsvariable `RUST_LOG` steuern.
  Ohne sie gilt `info` (Spec 0094 A3).

## 3. Speicherort und Format

- Die Logdatei liegt in einem plattformspezifischen Ordner:
  - macOS: `~/Library/Logs/Smart SSH/`
  - Windows: `%APPDATA%\Smart SSH\logs\`
  - Linux: `~/.local/state/smart-ssh/logs/`
- Format ist JSON Lines: eine Zeile pro Ereignis, mit Level, Zeitstempel,
  Nachricht und Feldern. So lässt sich gezielt nach Level oder einer
  Sitzungs- bzw. Anfrage-Kennung filtern, auch ohne die App.
- Die Datei rotiert täglich. Beim Start löscht die App Dateien, die älter
  als 14 Tage sind. Schlägt das Aufräumen fehl, startet die App trotzdem.

## 4. Was protokolliert wird

Zusammengehörige Zeilen eines KI-Anfrage-Zyklus tragen dieselbe
Anfrage-Kennung, Zeilen einer Sitzung dieselbe Sitzungs-Kennung.

1. **Ausgehende KI-Anfrage:** was an den Provider geht, erst **nach** der
   Redaction (Spec 0006), nie davor. Für Logs gilt dieselbe Redaction-Regel
   wie für die Anfrage selbst.
2. **Antwort des Providers:** Werkzeugaufrufe und Textabschnitte.
3. **Auswertung eines Werkzeugaufrufs:** bei Erfolg die erkannte Aktion; bei
   einem Fehler der Grund (Feld, erwarteter und tatsächlicher Typ), damit ein
   Fall wie eine ungültige Ziel-Kennung sofort erklärbar ist.
4. **Filter-Entscheidung:** Entscheidung und gegriffene Regel bzw.
   Hard-Blacklist-Eintrag (Spec 0009).
5. **SSH-Ausführung:** Exit-Code und Ausgabelängen.
6. **Sitzungs-Lebenszyklus:** Verbindungsaufbau und -abbau, Host-Key-Ereignisse,
   jeweils mit Grund bzw. Status.

Welche dieser Angaben auf welchem Level stehen, regelt Spec 0094: Auf `info`,
`warn` und `error` stehen nur inhaltsfreie Angaben (Kennungen, Entscheidung,
Regel, Längen, Exit-Code, Fehlercodes); der Inhalt selbst (Kommandotext,
Ausgabe, Kontext, Rohantwort) erscheint nur auf `debug`, redigiert.

## 5. Zugriff

- In den Einstellungen öffnet ein Klick den Log-Ordner im Dateimanager des
  Systems.
- Der Pfad aus §3 ist fest und dokumentiert; Werkzeuge mit Terminalzugriff
  können die Dateien direkt lesen.

## 6. Fehler in Werkzeugaufrufen

- Für „die Notiz des aktuell verbundenen Servers" muss die KI keine Kennung
  nennen oder formatieren: Ein Notiz-Vorschlag trägt stattdessen ein optionales
  Ziel `current_server` oder `current_server_group`. Fehlt es, gilt
  `current_server`. Die App löst die Kennung aus dem Sitzungskontext selbst
  auf.
- Ein Fehler beim Auswerten eines Werkzeugaufrufs beendet weder Verbindung
  noch Sitzung. Er erscheint als sichtbarer Hinweis im Chat; die Sitzung
  bleibt nutzbar.

## 7. Sicherheitszusagen

- Die Logdatei ist ab `info` keine Datensenke für Kommandotext, Ausgabe,
  Chat-, Notiz- oder Prompt-Inhalte (Spec 0094).
- Was auf `debug` steht, läuft vorher durch die Redaction.

## 8. Grenzen

- Dateien älterer Versionen können noch Inhalte enthalten; sie werden nach
  14 Tagen gelöscht.
- Die Datenbank (Ausführungsprotokoll, Chatverlauf) ist nicht Gegenstand
  dieser Spec.
