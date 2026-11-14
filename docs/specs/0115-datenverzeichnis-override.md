# Spec 0115 — Eigenes Datenverzeichnis pro Instanz

Status: umgesetzt
Zweck: Eine zweite Kopie der Desktop-App (z. B. eine duplizierte App zum Testen) arbeitet mit einem eigenen Datenverzeichnis und liest und schreibt nichts außerhalb davon. Das Fenster zeigt, welches Verzeichnis die Instanz benutzt.
Bezüge: ADR 0032 (Debug-Build-Datenverzeichnis und Override), Spec 0016 (Logging), Spec 0052 (Versionsanzeige), ADR 0105 (MCP-Einstellungen), `docs/datenpfade.md`.

## 1. Quellen und Vorrang

Das Datenverzeichnis wird beim Start genau einmal bestimmt, in dieser Reihenfolge (höchste zuerst):

1. Kommandozeilenargument `--data-dir <pfad>` oder `--data-dir=<pfad>`.
2. Umgebungsvariable `SMART_SSH_DATA_DIR`.
3. Zeigerdatei neben der App (Abschnitt 2).
4. Plattform-Standard (unverändert, auch der Debug-Suffix aus ADR 0032).

Ein leerer Wert gilt als nicht gesetzt. Unbekannte Argumente werden ignoriert. Auf Android gibt es keine Zeigerdatei.

## 2. Zeigerdatei

Eine Textdatei neben der App; die erste nicht leere Zeile ist der Pfad. `~` wird zum Home-Verzeichnis, ein relativer Pfad gilt relativ zum Ordner der Zeigerdatei.

- macOS: `<Ordner der .app>/<Bundle-Name ohne .app>.data-dir` (z. B. `Smart SSH Test.data-dir`). Das Bundle bleibt unverändert, seine Signatur gültig.
- Windows: `<Ordner der .exe>\<Name der .exe ohne Endung>.data-dir`.
- Linux: neben dem AppImage (`$APPIMAGE`), sonst neben der ausführbaren Datei, `<Name>.data-dir`.

Eine Kopie der App ohne Zeigerdatei benutzt den Standard.

## 3. Was im Verzeichnis liegt

Mit einem Override liegt alles darin, und die Instanz legt, liest und schreibt keine Datei außerhalb: Datenbank samt Host-Keys und Schlüsseldatei, Logs (`logs/`), `settings.json` (auch MCP-Einstellungen) und Sprachmodelle (`speech-models/`). Ohne Override bleiben alle Pfade wie bisher; es gibt keine Migration.

Nicht Teil des Verzeichnisses: Einträge im Schlüsselbund des Betriebssystems; sie werden weiterhin von allen Instanzen gemeinsam benutzt.

## 4. Fehlerverhalten

Lässt sich das gewählte Verzeichnis nicht anlegen oder benutzen, erscheint das Startfehlerfenster mit Pfad und Quelle (Argument, Umgebungsvariable oder Zeigerdatei). Die App weicht nie auf das Standardverzeichnis aus und legt dort nichts an.

## 5. Anzeige

- Mit einem Override zeigt die Titelzeile neben der Version „Daten: ~/Pfad“ (Home mit `~` gekürzt, voller Pfad als Tooltip). Ohne Override erscheint nichts.
- Einstellungen → Diagnose → Datenpfade listet die wirksamen Pfade, auch Logs, Einstellungsdatei und Sprachmodelle.
- Der Start-Logeintrag nennt die Quelle des Verzeichnisses, nicht den Inhalt der Zeigerdatei.

## 6. Mehrere Instanzen

Instanzen mit unterschiedlichen Verzeichnissen laufen gleichzeitig; das Fokussieren einer bereits laufenden Instanz ist mit Override aus, die Sperre je Verzeichnis bleibt. Ist der Port des MCP-Servers schon von der anderen Instanz belegt, zeigt die zweite den bestehenden MCP-Fehlerzustand und läuft weiter.
