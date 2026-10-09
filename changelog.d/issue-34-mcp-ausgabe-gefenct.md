### Sicherheit
- Kommandoausgaben und gelesene Dateien, die ein externer MCP-Client über
  `propose_command` und `read_remote_file` bekommt, sind jetzt als nicht
  vertrauenswürdiger Inhalt eingezäunt (`<stdout>`, `<stderr>`,
  `<remote_file>`) — genau wie im Kontext der eingebauten KI. Eine vom
  Server eingeschleuste Anweisung kann den Zaun nicht verlassen. Clients,
  die den Text bisher wörtlich ausgewertet haben, sehen ein geändertes
  Format.
