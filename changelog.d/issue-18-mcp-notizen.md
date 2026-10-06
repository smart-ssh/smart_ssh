### Sicherheit
- Server-Notizen, die ein externer MCP-Client über `get_server_notes` liest,
  sind jetzt redigiert (z. B. private Schlüssel, API-Tokens, das
  Sudo-Passwort des Servers) und als nicht vertrauenswürdiger Inhalt
  eingezäunt — genau wie die Notizen, die die eingebaute KI bekommt.
