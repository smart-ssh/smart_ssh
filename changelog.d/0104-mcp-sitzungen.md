### Geändert
- Aktionen, die ein externes Tool über MCP anfragt, laufen jetzt in einer
  eigenen Sitzung mit eigener Verbindung und eigenem Tab je Server und Tool
  („Claude Code @ web-01“, MCP-Abzeichen). Der eigene Tab, das Terminal und
  der Chat bleiben unberührt, und der MCP-Tab übernimmt nicht den Fokus. Eine
  wartende Bestätigung zeigt der Tab mit einem Abzeichen an.

### Sicherheit
- MCP-Ausgaben gelangen nicht mehr in den KI-Verlauf der eigenen Sitzung,
  und der eigene Chat nicht in die MCP-Sitzung. Schließen des MCP-Tabs
  lehnt eine wartende MCP-Bestätigung sofort ab.
