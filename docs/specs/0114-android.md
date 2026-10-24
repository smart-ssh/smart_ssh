# Spec 0114 — Android (technischer Test)

Status: Grundlage umgesetzt; Bedienung per Touch, Schlüsselspeicher, Hintergrundsitzungen, Dateiauswahl und Spracheingabe folgen in eigenen Änderungen
Zweck: Die App läuft als Android-App (APK) auf Telefonen und im Emulator. Dieser Stand ist ein technischer Test: Entsperren, Serverliste, SSH-Verbindung, KI-Chat und Terminal funktionieren; nicht alles vom Desktop ist vorhanden.
Bezüge: Spec 0113 (Schmales Layout), Spec 0101 (Master-Passwort), Spec 0049 (Titelleiste).

## 1. Was gleich bleibt

- Erster Start legt die Datenbank an; Entsperren, Server anlegen, SSH-Verbindung mit Passwort, Terminal (Ausgabe und Eingabe), KI-Chat über HTTPS und der MCP-Server (nur auf 127.0.0.1) verhalten sich wie auf dem Desktop.
- Filter-, Risiko- und Bestätigungslogik gelten unverändert.

## 2. Was anders ist

- **Layout:** Es gilt immer das schmale Layout (Spec 0113), unabhängig von der Breite. Es gibt keine Titelleiste mit Fenster-Steuerung und kein Ziehen oder Maximieren des Fensters; die Kopfzeile reserviert dafür keinen Platz.
- **Schlüsselspeicher:** Der Datenbankschlüssel liegt standardmäßig im Android-Schlüsselspeicher (Keystore); die Datenbank öffnet sich nach einem Neustart ohne Eingabe. Lässt sich der Schlüsselspeicher nicht öffnen, bietet die App den Master-Passwort-Modus an (Spec 0101, Abschnitt 8). Das Master-Passwort kann auch sonst gewählt werden.
- **Lokaler Pseudo-Server:** Er erscheint nicht in der Serverliste.
- **Ordner:** „Ordner hochladen“ und „Herunterladen nach …“ für Ordner werden nicht angeboten.
- **Dateien in die Zwischenablage:** „Datei kopieren“ und das Einfügen lokaler Dateien aus der Zwischenablage werden nicht angeboten. Text kopieren funktioniert.
- **Mehrfachstart:** Das System hält höchstens eine Instanz; eine Prüfung wie auf dem Desktop entfällt.
- **Startfehler:** Ein Fehler beim Start erscheint nicht in einem System-Dialog, sondern wird ins Log geschrieben, danach beendet sich die App. Rückfragen beim Start gelten als „Abbrechen“.

## 3. Ablage

- Datenbank, Logs, Bearbeitungskopien und Downloads liegen im app-privaten Speicher der App; kein Pfad hängt vom Heimverzeichnis ab, ein fehlendes `HOME` verursacht keinen Absturz.
