### Behoben
- Scheitert der Start an einer Datenbank aus einer neueren Version, ist die
  Datenbank vollständig geschlossen, bevor der Fehlerdialog erscheint. Es
  bleiben keine `-wal`/`-shm`-Dateien neben der Datenbank zurück, und unter
  Windows lässt sich die Datei sofort umbenennen.
