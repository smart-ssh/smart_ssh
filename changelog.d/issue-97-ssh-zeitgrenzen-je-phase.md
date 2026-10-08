### Geändert
- Der SSH-Verbindungsaufbau hat jetzt getrennte Zeitgrenzen je Server in der
  Kette: 10 s für Verbindung und Handshake, 60 s für die Anmeldung. Ein
  Hardware-Schlüssel mit Berührung oder ein Agent-Schlüssel mit Bestätigung
  bricht nicht mehr nach 10 s mit einer Zeitüberschreitung ab, und Ketten über
  Jump-Hosts bekommen je Hop eigene Zeit. Ein Server, der nicht antwortet,
  scheitert weiterhin nach 10 s.
