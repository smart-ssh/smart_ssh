### Behoben
- Smart SSH lässt sich nicht mehr zweimal mit demselben Datenverzeichnis
  starten. Ein zweiter Start holt das bereits offene Fenster nach vorn. Hält
  ein anderer Prozess die Daten (etwa ein anderer Build mit demselben
  Datenverzeichnis), erscheint die Meldung „Smart SSH läuft bereits mit
  diesem Datenverzeichnis“, und die Datenbank bleibt unberührt. Vorher
  konnten zwei Instanzen dieselbe Datenbank gleichzeitig öffnen, migrieren
  und verschlüsseln.
