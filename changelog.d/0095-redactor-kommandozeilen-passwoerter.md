### Sicherheit
- Passwörter, die als Argument eines Kommandozeilenprogramms übergeben
  werden, werden jetzt geschwärzt, bevor ein Kommando oder seine Ausgabe in
  den KI-Kontext, ins Protokoll, in den Chat oder zu einem MCP-Client geht:
  `mysql`/`mariadb`/`mysqldump`/`mysqladmin` mit angehängtem `-p`, `sshpass -p`,
  `curl -u benutzer:passwort`, `htpasswd -b`, `redis-cli -a`,
  `smbclient -U benutzer%passwort` sowie `openssl -pass pass:…` und
  `openssl -k`. Programm, Schalter und Benutzername bleiben lesbar — nur der
  Wert verschwindet.
- Zusätzlich erkannt: Twilio-API-Schlüssel, SendGrid-Schlüssel, die
  Schlüssel in Azure-Verbindungszeichenketten (`AccountKey=`,
  `SharedAccessKey=`), Argon2- und phpass-Passwort-Hashes sowie JSON-Web-Token
  ohne `Bearer`-Präfix.
