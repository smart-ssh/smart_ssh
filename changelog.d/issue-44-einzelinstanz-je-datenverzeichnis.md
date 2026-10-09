### Behoben
- Zwei Smart-SSH-Instanzen mit verschiedenen Datenverzeichnissen (über
  `SMART_SSH_DATA_DIR`) laufen jetzt nebeneinander. Vorher holte der zweite
  Start die erste Instanz nach vorn und beendete sich, nachdem er seine
  eigene Datenbank schon geöffnet hatte. Ein zweiter Start mit dem
  Standard-Datenverzeichnis holt das offene Fenster weiterhin nach vorn.
