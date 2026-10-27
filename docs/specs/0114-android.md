# Spec 0114 — Android (technischer Test)

Status: Grundlage umgesetzt, Dateiauswahl, Hintergrundsitzungen und Spracheingabe umgesetzt; Bedienung per Touch und Schlüsselspeicher folgen in eigenen Änderungen
Zweck: Die App läuft als Android-App (APK) auf Telefonen und im Emulator. Dieser Stand ist ein technischer Test: Entsperren, Serverliste, SSH-Verbindung, KI-Chat und Terminal funktionieren; nicht alles vom Desktop ist vorhanden.
Bezüge: Spec 0113 (Schmales Layout), Spec 0101 (Master-Passwort), Spec 0049 (Titelleiste).

## 1. Was gleich bleibt

- Erster Start legt die Datenbank an; Entsperren, Server anlegen, SSH-Verbindung mit Passwort, Terminal (Ausgabe und Eingabe), KI-Chat über HTTPS und der MCP-Server (nur auf 127.0.0.1) verhalten sich wie auf dem Desktop.
- Filter-, Risiko- und Bestätigungslogik gelten unverändert.

## 2. Was anders ist

- **Layout:** Es gilt immer das schmale Layout (Spec 0113), unabhängig von der Breite. Es gibt keine Titelleiste mit Fenster-Steuerung und kein Ziehen oder Maximieren des Fensters; die Kopfzeile reserviert dafür keinen Platz.
- **Schlüsselspeicher:** Der Datenbankschlüssel liegt standardmäßig im Android-Schlüsselspeicher (Keystore); die Datenbank öffnet sich nach einem Neustart ohne Eingabe. Lässt sich der Schlüsselspeicher nicht öffnen, bietet die App den Master-Passwort-Modus an (Spec 0101, Abschnitt 8). Das Master-Passwort kann auch sonst gewählt werden.
- **Systemleisten und Tastatur:** Die App zeichnet randlos, hält ihre Inhalte aber frei von Statusleiste, Navigations-/Gestenleiste, Display-Aussparung und eingeblendeter Tastatur: Kopfzeile und Menü-Knopf liegen vollständig unterhalb der Statusleiste und sind bedienbar, das unterste Bedienelement einer vollen Ansicht liegt oberhalb der Gestenleiste (Hoch- und Querformat), bei offener Tastatur bleiben das fokussierte Eingabefeld und die Terminal-Eingabezeile sichtbar. Der Bereich hinter den Leisten hat die Hintergrundfarbe der App; Statusleiste und Navigationsleiste sowie der Bereich dahinter folgen der Darstellungseinstellung (System/Hell/Dunkel; „System“ = Systemdesign des Geräts): im hellen Design dunkle Symbole auf dem hellen App-Hintergrund, im dunklen Design helle Symbole auf dunklem Grund. Ein Wechsel in den Einstellungen oder – bei „System“ – am Gerät wirkt sofort ohne Neustart; beim Kaltstart im hellen Design erscheint der Bereich hinter den Leisten nicht kurz dunkel. Auf dem Desktop ändert sich nichts.
- **Lokaler Pseudo-Server:** Er erscheint nicht in der Serverliste.
- **Ordner:** „Ordner hochladen“ und „Herunterladen nach …“ für Ordner werden nicht angeboten.
- **Dateien in die Zwischenablage:** „Datei kopieren“ und das Einfügen lokaler Dateien aus der Zwischenablage werden nicht angeboten. Text kopieren funktioniert.
- **Dateiauswahl:** Hochladen, Schlüssel-Import, Import der SSH-Konfiguration und alle Dialoge zum Wählen einer Datei nutzen den Auswahldialog des Systems; Speichern (Herunterladen einer Datei, Exporte, Diagnosepaket) nutzt dessen Speichern-Dialog. „Herunterladen“ für eine Datei fragt daher immer nach dem Ziel.
- **Keine Dateimanager-Funktionen:** „Im Ordner anzeigen“ und das Ablegen von Dateien per Drag & Drop in den Dateibrowser gibt es nicht.
- **Schlüsseldatei:** Ein Pfad zu einer Schlüsseldatei kann auf Android nicht gemerkt werden. „Schlüsseldatei“ wird für neue Server nicht angeboten; einen Schlüssel wählt man über „Private Key“, der gewählte Inhalt wird übernommen. Ein bereits gespeicherter Server mit Schlüsseldatei behält die Anmeldeart.
- **Lokal öffnen:** Es gibt kein externes Programm; die Datei öffnet sich in einem Editor der App (Textdateien bis 2 MiB). Beim Speichern gilt der übliche Ablauf mit Vergleich und Bestätigung.
- **SSH-Konfiguration:** Beim Import aus einer gewählten Datei werden `Include`-Verweise nicht verfolgt.
- **Mehrfachstart:** Das System hält höchstens eine Instanz; eine Prüfung wie auf dem Desktop entfällt.
- **Startfehler:** Ein Fehler beim Start erscheint nicht in einem System-Dialog, sondern wird ins Log geschrieben, danach beendet sich die App. Rückfragen beim Start (Dialoge D1–D4, die zweiten Bestätigungen und die Hinweise, Spec 0101) erscheinen im Fenster, genau wie auf dem Desktop; die App wählt nie selbst eine Antwort und beendet sich nicht von allein; „Beenden“ in einem Dialog bricht den Start ab und zeigt die Maske mit „Erneut versuchen“ und „Beenden“, erst dessen „Beenden“ schließt die App.
- **Spracheingabe:** Sie arbeitet wie auf dem Desktop (Spec 0108), vollständig lokal: Der Mikrofon-Knopf nimmt auf, der Live-Text erscheint, „Stopp“ fügt den endgültigen Text ein. Die Modelle liegen im app-privaten Speicher; die Größenwarnung vor dem Download gilt unverändert. Die Android-Berechtigung „Mikrofon“ wird beim ersten Start einer Aufnahme (nach dem Modell-Download) abgefragt. Wird sie verweigert, erscheint der Hinweis „Zugriff auf das Mikrofon verweigert“ mit dem Verweis auf die Systemeinstellungen; die App stürzt nicht ab und die Aufnahme beginnt nicht. Die nächste Aufnahme fragt erneut bzw. folgt der Systemeinstellung.

## 2a. Terminal mit Zusatztastenreihe

- Auf Touch-Geräten (Android sowie jedes Gerät mit grobem Zeiger) zeigt der Terminal-Tab unter dem Terminal eine Tastenreihe: `Esc`, `Ctrl`, `Alt`, `Tab`, `↑ ↓ ← →`, `Home`, `End`, `PgUp`, `PgDn`, `|`, `~`, `/`, `-`. Auf dem Desktop mit Maus fehlt sie. Jede Taste ist mindestens 44 × 44 px groß und hat in allen Sprachen eine Beschriftung für Screenreader.
- Die Tasten senden dieselben Zeichenfolgen wie die entsprechenden physischen Tasten im Terminal (Pfeiltasten und `Home`/`End` folgen dem Anwendungsmodus der Programme, z. B. in `vim`).
- `Ctrl` und `Alt` sind einrastend für genau eine Eingabe: Nach dem Antippen (die Taste erscheint gedrückt) wirken sie auf die nächste Taste der Bildschirmtastatur oder der Reihe und lösen sich danach. `Ctrl` + `c` sendet `Ctrl+C`, `Alt` + `b` sendet `Esc b`. Eingaben aus mehreren Zeichen (Einfügen, Diktat) bleiben unverändert.
- Die Tasten der Reihe nehmen der Bildschirmtastatur den Fokus nicht weg.
- Öffnet sich die Bildschirmtastatur, schrumpft die App auf den sichtbaren Bereich, das Terminal passt seine Größe an und die Cursorzeile bleibt sichtbar; beim Schließen wächst es zurück.

## 3. Ablage

- Datenbank, Logs, Bearbeitungskopien und Downloads liegen im app-privaten Speicher der App; kein Pfad hängt vom Heimverzeichnis ab, ein fehlendes `HOME` verursacht keinen Absturz.
- Für Übertragungen mit dem Auswahldialog legt die App eine Kopie im app-privaten Speicher an; sie wird nach der Übertragung bzw. dem Lesen wieder gelöscht.

## 4. Hintergrund

- Offene SSH-Sitzungen laufen weiter, wenn man zu einer anderen App wechselt oder den Bildschirm ausschaltet. Solange mindestens eine Sitzung offen ist, läuft ein Vordergrunddienst mit einer dauerhaften Benachrichtigung („Smart SSH – 2 sessions active“). Er startet mit der ersten geöffneten Sitzung und endet mit dem Schließen der letzten.
- Die Benachrichtigung hat die Aktion „Disconnect all“, die alle Sitzungen trennt; ein Tippen auf die Benachrichtigung öffnet die App.
- Bei der ersten Sitzung fragt die App nach der Benachrichtigungs-Berechtigung (Android 13+). Wird sie verweigert, läuft der Dienst trotzdem (sichtbar im System-Taskmanager), und die App zeigt einmalig einen Hinweis. Danach wird nicht erneut gefragt.
- Eine Sitzung ohne laufende Arbeit hält das Gerät nicht wach. Nur solange ein Befehl läuft oder eine Dateiübertragung aktiv ist, wird ein Teil-Wake-Lock gehalten.
- Bricht die Verbindung beim Netzwechsel (WLAN ↔ Mobilfunk) ab, gilt das automatische Neuverbinden (Spec 0107). Der SSH-Keepalive bleibt bei 15 Sekunden.
- Kehrt man zur App zurück, wird der Verbindungsstatus der offenen Tabs neu vom Backend gelesen.
