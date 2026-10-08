### Sicherheit
- Ein leeres MCP-Token wird nicht mehr akzeptiert: Der MCP-Server lehnt dann
  jede Anfrage ab, und die App ersetzt ein leer gespeichertes Token beim
  Start durch ein neues. Externe Tools, die mit einem leeren Token
  eingerichtet waren, brauchen das neue Token aus den MCP-Einstellungen.
