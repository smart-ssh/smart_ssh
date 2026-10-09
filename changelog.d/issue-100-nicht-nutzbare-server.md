### Behoben
- Ein Server, dessen gespeicherte Anmeldeart diese Version nicht lesen kann
  (z. B. nach dem Zurückgehen von einer neueren Version), lässt die
  Serverliste nicht mehr leer bleiben. Er erscheint als „nicht nutzbar" mit
  Grund und lässt sich samt den Secrets löschen, die diese Version kennt
  (Secrets, die nur eine neuere Version angelegt hat, können zurückbleiben);
  alle anderen Server funktionieren normal, und seine gespeicherten Daten bleiben unverändert.
