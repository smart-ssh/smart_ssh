### Behoben

- Eine beschädigte, fremde oder unlesbare Schlüsseldatei neben der Datenbank
  sperrt die App nicht mehr dauerhaft aus. Vor der Entsperrung gibt es dafür
  jetzt „Neu anfangen“ — und zwar nur dann, wenn wirklich kein Passwort mehr
  passt. Ist die Datei in Ordnung und bloß das Passwort falsch, bleibt es bei
  der erneuten Eingabe.
- Beim Ändern des Master-Passworts wird die neue Schlüsseldatei geprüft,
  **bevor** die alte ersetzt wird. Scheitert die Prüfung, gilt weiter das
  alte Passwort statt keines. Halbe Schreibversuche bleiben nicht liegen.
- Solange die App gesperrt ist, sind auch die Einstellungsdateien und die
  Systemdienste nicht erreichbar, nicht nur die eigenen Befehle.
- Eine Rückfrage beim Start, die niemand sieht, endet jetzt mit einer
  Meldung statt mit einem wartenden Fenster.
- Fehlermeldungen beim Start benennen den Zustand richtig: Sie behaupten
  nicht mehr „es ist nichts verändert“, wenn Dateien schon zur Seite gelegt
  wurden, und sie nennen die Datei, um die es geht.
