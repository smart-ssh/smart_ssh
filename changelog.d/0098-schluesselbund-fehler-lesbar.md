### Behoben

- Schlägt ein Zugriff auf den Systemschlüsselbund fehl, obwohl er beim Start
  erreichbar war — weil er gesperrt ist oder der Zugriff abgelehnt wurde —,
  dann erscheint jetzt eine erklärende Meldung in der Sprache der App statt
  des rohen englischen Texts der Schlüsselbund-Bibliothek („No default store
  has been set …"). Das betrifft das Speichern und Lesen von API-Keys,
  Server-Passwörtern, Passphrasen und Sudo-Passwörtern.
- Beim Verbindungsaufbau und beim Verbindungstest wird ein solcher Fehler
  nicht mehr als „Netzwerkfehler" oder „Zugangsdaten konnten nicht aufgelöst
  werden" angezeigt. Die Meldung nennt den Schlüsselbund als Ursache und —
  bei einer Kette über Jump-Hosts — den Rechner, dessen Zugangsdaten nicht
  gelesen werden konnten.
- Ein einzelner fehlgeschlagener Zugriff gilt nicht länger als „Schlüsselbund
  nicht verfügbar". Die Meldung behauptet das nicht mehr, und der nächste
  Zugriff gelingt wieder, sobald der Schlüsselbund antwortet. Fehlt der
  Eintrag im Schlüsselbund schlicht, bleibt es weiterhin bei der bisherigen
  Meldung „kein Eintrag gefunden" — das ist kein Fehler des Schlüsselbunds.
