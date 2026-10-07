### Sicherheit
- Linux: Die Datei mit den MCP-Einstellungen (`settings.json`) wird jetzt
  tatsächlich auf Rechte `0600` gesetzt. Bisher zielte die Härtung auf ein
  anderes Verzeichnis, und die Datei blieb für andere Nutzer lesbar.
  Bestehende Dateien werden beim nächsten Speichern einer MCP-Einstellung
  korrigiert.
