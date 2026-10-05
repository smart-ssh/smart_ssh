### Behoben

- Eine beschädigte oder fremde Schlüsseldatei neben der Datenbank sperrt die
  App nicht mehr dauerhaft aus. Vor der Entsperrung gibt es dafür jetzt „Neu
  anfangen“ — und zwar nur dann, wenn die Datei gelesen wurde und wirklich
  kein Passwort mehr passen kann. Ist die Datei in Ordnung und bloß das
  Passwort falsch, bleibt es bei der erneuten Eingabe.
- Lässt sich die Schlüsseldatei gerade **nicht lesen** — fehlende Rechte, ein
  Lesefehler, ein anderes Programm hält sie offen —, wird nichts verändert
  und nichts umbenannt. Die App sagt das und lässt es erneut versuchen; ein
  neuer Schlüssel entsteht dabei nie. Vorher bot sie in dieser Lage „Neu
  anfangen“ an, obwohl die Datei unversehrt war.
- Der Wechsel vom Master-Passwort zurück auf den Schlüsselbund überschreibt
  keinen fremden Schlüssel mehr, der dort schon liegt — etwa den einer
  zweiten Installation. Ohne ausdrückliche Bestätigung bleibt alles, wie es
  war.
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
