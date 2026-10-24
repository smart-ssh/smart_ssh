# Spec 0114 — Android (technischer Test)

Status: Grundlage umgesetzt; Bedienung per Touch, Schlüsselspeicher, Dateiauswahl und Spracheingabe folgen in eigenen Änderungen
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
- **Mehrfachstart:** Das System hält höchstens eine Instanz; eine Prüfung wie auf dem Desktop entfällt.
- **Startfehler:** Ein Fehler beim Start erscheint nicht in einem System-Dialog, sondern wird ins Log geschrieben, danach beendet sich die App. Rückfragen beim Start gelten als „Abbrechen“.

## 3. Ablage

- Datenbank, Logs, Bearbeitungskopien und Downloads liegen im app-privaten Speicher der App; kein Pfad hängt vom Heimverzeichnis ab, ein fehlendes `HOME` verursacht keinen Absturz.

## 4. Hintergrund

- Offene SSH-Sitzungen laufen weiter, wenn man zu einer anderen App wechselt oder den Bildschirm ausschaltet. Solange mindestens eine Sitzung offen ist, läuft ein Vordergrunddienst mit einer dauerhaften Benachrichtigung („Smart SSH – 2 sessions active“). Er startet mit der ersten geöffneten Sitzung und endet mit dem Schließen der letzten.
- Die Benachrichtigung hat die Aktion „Disconnect all“, die alle Sitzungen trennt; ein Tippen auf die Benachrichtigung öffnet die App.
- Bei der ersten Sitzung fragt die App nach der Benachrichtigungs-Berechtigung (Android 13+). Wird sie verweigert, läuft der Dienst trotzdem (sichtbar im System-Taskmanager), und die App zeigt einmalig einen Hinweis. Danach wird nicht erneut gefragt.
- Eine Sitzung ohne laufende Arbeit hält das Gerät nicht wach. Nur solange ein Befehl läuft oder eine Dateiübertragung aktiv ist, wird ein Teil-Wake-Lock gehalten.
- Bricht die Verbindung beim Netzwechsel (WLAN ↔ Mobilfunk) ab, gilt das automatische Neuverbinden (Spec 0107). Der SSH-Keepalive bleibt bei 15 Sekunden.
- Kehrt man zur App zurück, wird der Verbindungsstatus der offenen Tabs neu vom Backend gelesen.
