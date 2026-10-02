### Changed

- Das Token für den MCP-Server liegt jetzt in der verschlüsselten
  Datenbank statt im Klartext in `settings.json`. Ein vorhandenes Token
  wird beim ersten Start unverändert übernommen und danach aus der Datei
  entfernt — bestehende MCP-Client-Konfigurationen funktionieren weiter.
