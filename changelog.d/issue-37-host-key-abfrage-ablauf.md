### Geändert
- Läuft eine offene Abfrage für einen unbekannten oder geänderten Host-Key
  im Hintergrund ab (etwa bei einer über MCP gestarteten Verbindung),
  schließt sie sich jetzt von selbst, und ein Hinweis meldet, dass die
  Verbindung nicht aufgebaut und dem Schlüssel nicht vertraut wurde. Bisher
  blieb die Abfrage offen, und eine späte Entscheidung endete mit einer
  Fehlermeldung.
