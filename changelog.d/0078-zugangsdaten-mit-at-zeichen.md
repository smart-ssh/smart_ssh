### Behoben
- Zugangsdaten in einer Verbindungs-URL werden jetzt auch dann vollständig
  geschwärzt, wenn Passwort oder Benutzername ein unkodiertes `@` enthalten
  (etwa `postgres://app:Xy9@kLm2@db/prod` oder die bei mehreren gehosteten
  Datenbanken vorgeschriebene Schreibweise `benutzer@mandant`). Das gilt für
  alles, was an das KI-Modell geht, und für alles, was in der
  Gesprächshistorie gespeichert wird.
- Ein Passwort-Parameter im Query-String einer Verbindungs-URL ohne Pfad
  (`redis://cache:6379?password=…`) wird ebenfalls vollständig geschwärzt.
