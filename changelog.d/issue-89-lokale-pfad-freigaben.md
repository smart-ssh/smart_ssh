### Sicherheit
- Der Dateibrowser liest für Upload, Überschreib-Vorschau und „Lokal öffnen“
  nur noch lokale Dateien, die der Nutzer für die Sitzung im Öffnen-Dialog
  gewählt oder auf das Fenster gezogen hat, oder die eigene
  Bearbeitungskopie der Sitzung. Andere Pfade — auch über `..` oder einen
  Symlink aus einem freigegebenen Ordner hinaus — werden abgelehnt, bevor
  etwas gelesen wird. Freigaben enden beim Trennen der Sitzung.
