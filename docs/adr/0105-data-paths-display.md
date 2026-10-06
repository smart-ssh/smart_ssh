# ADR 0105 — Anzeige der Datenpfade: Quellen, Ordner öffnen, Edition-Einträge

Status: akzeptiert
Betrifft: Issue #16, Spec 0063, ADR 0032

## Problem

Issue #16 verlangt einen Befehl, der die wirksamen Pfade (Datenbank, Logs,
Host-Keys, Schlüsseldatei des Master-Passworts, MCP-Einstellungen) liefert,
ohne zweite Quelle der Wahrheit, plus „Ordner öffnen" und einen Weg, über
den eine Edition weitere Pfade ergänzt. Drei Punkte ließ das Issue offen.

## Entscheidung

1. **MCP-Einstellungen über `tauri_plugin_store::resolve_store_path`, nicht
   über `app_config_dir`.** Der Store löst `settings.json` gegen
   `BaseDirectory::AppData` auf. Auf macOS und Windows ist das derselbe Ort
   wie `app_config_dir`, auf Linux nicht (`~/.local/share/<id>` statt
   `~/.config/<id>`). Die Anzeige nutzt dieselbe Funktion wie der Store,
   damit sie den Ort nennt, an dem die Datei tatsächlich liegt.
2. **„Ordner öffnen" nimmt einen Bezeichner, keinen Pfad.**
   `open_data_path_folder(id)` ermittelt die Liste neu und öffnet den Ordner
   zum Eintrag (`app_logic::data_paths::open_folder_target`). Ein Befehl mit
   Pfad-Parameter würde jedem Code im Webview erlauben, beliebige Pfade im
   Dateimanager zu öffnen (vgl. `read_credential_file`, Spec 0013). Der
   Befehl legt keinen Ordner an; fehlt er, ist das ein sichtbarer Fehler.
   Bei Dateien öffnet er den enthaltenden Ordner, bei den Logs den
   Log-Ordner selbst.
3. **Edition-Einträge über `Wiring::extra_data_paths`.** Eine Edition gibt
   Bezeichner, Anzeigetext, Pfad und „ist Verzeichnis" mit. Der Anzeigetext
   kommt von der Edition, weil der Übersetzungskatalog des Frontends
   editionsspezifische Einträge nicht kennt. Community liefert keine.

Außerdem nutzt der Diagnose-Export für den Host-Key-Pfad jetzt
`startup_error_messages::host_key_store_path` statt einer eigenen
Ableitung, damit Export, Einstellungen und Speicher denselben Pfad nennen.

## Konsequenzen

- `Wiring` hat ein neues öffentliches Feld; jede Edition muss es setzen
  (Community: `Vec::new()`).
- Der Diagnose-Export enthält weiterhin nur Datenbank, Logs und Host-Keys.
  Die Einstellungen zeigen zusätzlich Schlüsseldatei und MCP-Einstellungen;
  die gemeinsamen drei Pfade stimmen überein, weil beide dieselben
  Funktionen aufrufen.
