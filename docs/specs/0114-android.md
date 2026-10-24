# Spec 0114 — Android (technischer Test)

Status: Grundlage umgesetzt; Bedienung per Touch, Schlüsselspeicher, Hintergrundsitzungen und Spracheingabe folgen in eigenen Änderungen
Zweck: Die App läuft als Android-App (APK) auf Telefonen und im Emulator. Dieser Stand ist ein technischer Test: Entsperren, Serverliste, SSH-Verbindung, KI-Chat und Terminal funktionieren; nicht alles vom Desktop ist vorhanden.
Bezüge: Spec 0113 (Schmales Layout), Spec 0101 (Master-Passwort), Spec 0049 (Titelleiste).

## 1. Was gleich bleibt

- Erster Start legt die Datenbank an; Entsperren, Server anlegen, SSH-Verbindung mit Passwort, Terminal (Ausgabe und Eingabe), KI-Chat über HTTPS und der MCP-Server (nur auf 127.0.0.1) verhalten sich wie auf dem Desktop.
- Filter-, Risiko- und Bestätigungslogik gelten unverändert.

## 2. Was anders ist

- **Layout:** Es gilt immer das schmale Layout (Spec 0113), unabhängig von der Breite. Es gibt keine Titelleiste mit Fenster-Steuerung und kein Ziehen oder Maximieren des Fensters; die Kopfzeile reserviert dafür keinen Platz.
- **Schlüsselspeicher:** Bis der Android-Schlüsselspeicher angebunden ist, startet die App im Master-Passwort-Modus (Spec 0101).
- **Lokaler Pseudo-Server:** Er erscheint nicht in der Serverliste.
- **Ordner:** „Ordner hochladen“ und „Herunterladen nach …“ für Ordner werden nicht angeboten.
- **Dateien in die Zwischenablage:** „Datei kopieren“ und das Einfügen lokaler Dateien aus der Zwischenablage werden nicht angeboten. Text kopieren funktioniert.
- **Dateiauswahl:** Hochladen, Schlüssel-Import, Import der SSH-Konfiguration und alle Dialoge zum Wählen einer Datei nutzen den Auswahldialog des Systems; Speichern (Herunterladen einer Datei, Exporte, Diagnosepaket) nutzt dessen Speichern-Dialog. „Herunterladen“ für eine Datei fragt daher immer nach dem Ziel.
- **Keine Dateimanager-Funktionen:** „Im Ordner anzeigen“ und das Ablegen von Dateien per Drag & Drop in den Dateibrowser gibt es nicht.
- **Schlüsseldatei:** Ein Pfad zu einer Schlüsseldatei kann auf Android nicht gemerkt werden. „Schlüsseldatei“ wird für neue Server nicht angeboten; einen Schlüssel wählt man über „Private Key“, der gewählte Inhalt wird übernommen. Ein bereits gespeicherter Server mit Schlüsseldatei behält die Anmeldeart.
- **Lokal öffnen:** Es gibt kein externes Programm; die Datei öffnet sich in einem Editor der App (Textdateien bis 2 MiB). Beim Speichern gilt der übliche Ablauf mit Vergleich und Bestätigung.
- **SSH-Konfiguration:** Beim Import aus einer gewählten Datei werden `Include`-Verweise nicht verfolgt.
- **Mehrfachstart:** Das System hält höchstens eine Instanz; eine Prüfung wie auf dem Desktop entfällt.
- **Startfehler:** Ein Fehler beim Start erscheint nicht in einem System-Dialog, sondern wird ins Log geschrieben, danach beendet sich die App. Rückfragen beim Start gelten als „Abbrechen“.

## 3. Ablage

- Datenbank, Logs, Bearbeitungskopien und Downloads liegen im app-privaten Speicher der App; kein Pfad hängt vom Heimverzeichnis ab, ein fehlendes `HOME` verursacht keinen Absturz.
- Für Übertragungen mit dem Auswahldialog legt die App eine Kopie im app-privaten Speicher an; sie wird nach der Übertragung bzw. dem Lesen wieder gelöscht.
