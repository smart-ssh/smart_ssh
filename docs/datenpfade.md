# Wo Smart SSH seine Daten ablegt

Die Pfade der laufenden Instanz zeigt die App selbst unter
**Einstellungen → Diagnose → Datenpfade**: jeden Pfad zum Kopieren und mit
„Ordner öffnen". Diese Seite listet die Standardorte je Plattform.

## Standardpfade (Release-Build)

| Was | macOS | Linux | Windows |
|---|---|---|---|
| Datenbank | `~/Library/Application Support/Smart SSH/smart-ssh.db` | `~/.local/share/smart-ssh/smart-ssh.db` | `%APPDATA%\Smart SSH\smart-ssh.db` |
| Host-Keys | `~/Library/Application Support/Smart SSH/host_keys.json` | `~/.local/share/smart-ssh/host_keys.json` | `%APPDATA%\Smart SSH\host_keys.json` |
| Schlüsseldatei des Master-Passworts | `~/Library/Application Support/Smart SSH/smart-ssh.db.master-key` | `~/.local/share/smart-ssh/smart-ssh.db.master-key` | `%APPDATA%\Smart SSH\smart-ssh.db.master-key` |
| Logs (Ordner) | `~/Library/Logs/Smart SSH/` | `~/.local/state/smart-ssh/logs/` | `%APPDATA%\Smart SSH\logs\` |
| MCP- und App-Einstellungen | `~/Library/Application Support/com.smartssh.desktop/settings.json` | `~/.local/share/com.smartssh.desktop/settings.json` | `%APPDATA%\com.smartssh.desktop\settings.json` |
| Sitzungen | in der Datenbank | in der Datenbank | in der Datenbank |

Hinweise:

- **Linux:** `~/.local/share` steht für `$XDG_DATA_HOME`, `~/.local/state`
  für `$XDG_STATE_HOME`, falls gesetzt.
- **Windows:** `%APPDATA%` ist der Roaming-Ordner
  (`C:\Users\<Name>\AppData\Roaming`).
- **Schlüsseldatei des Master-Passworts:** existiert nur, wenn ein
  Master-Passwort eingerichtet ist (Spec 0101). Ohne Master-Passwort liegt
  der Datenbankschlüssel im Schlüsselbund des Betriebssystems.
- **Sperrdatei `smart-ssh.lock`** liegt neben der Datenbank. Solange Smart
  SSH läuft, hält es darauf eine Sperre des Betriebssystems, damit kein
  zweiter Prozess dieselben Daten öffnet (Issue #19). Die Datei selbst ist
  leer und darf liegen bleiben. Eine übrig gebliebene Datei blockiert
  keinen Start, nur ein laufender Prozess tut das.
- **Sitzungen** (Chat-Verlauf, Prompt-Historie, Protokoll) haben keine
  eigene Datei, sie liegen verschlüsselt in der Datenbank.
- `settings.json` enthält neben den MCP-Einstellungen auch weitere
  App-Einstellungen (z. B. Sprache, Layout). Das MCP-Token liegt nicht dort,
  sondern in der Datenbank.

## Entwicklungs-Build

Ein Debug-Build (`cargo tauri dev`) nutzt ein eigenes Datenverzeichnis, damit
er die Datenbank einer installierten Version nicht verändert (ADR 0032):
`Smart SSH (dev)` statt `Smart SSH` auf macOS und Windows, `smart-ssh-dev`
statt `smart-ssh` auf Linux. Das betrifft Datenbank, Host-Keys und
Schlüsseldatei; Logs und `settings.json` liegen am selben Ort wie oben.

## Eigenes Datenverzeichnis: `SMART_SSH_DATA_DIR`

Ist die Umgebungsvariable `SMART_SSH_DATA_DIR` gesetzt und nicht leer, liegen
**Datenbank, Host-Keys und Schlüsseldatei** in diesem Verzeichnis — in
Release- und Debug-Builds gleichermaßen, ohne Debug-Suffix:

```sh
SMART_SSH_DATA_DIR=/pfad/zu/testdaten smart-ssh
```

Logs und `settings.json` bleiben an ihrem Standardort. Ein leerer Wert gilt
als nicht gesetzt. Die Anzeige in den Einstellungen zeigt immer die
tatsächlich wirksamen Pfade, also auch den Override.
