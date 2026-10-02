### Hinzugefügt

- Die lokale Datenbank ist jetzt **vollständig verschlüsselt** (SQLCipher).
  Bisher lagen nur Chatverlauf, Ausführungsprotokoll und Eingabe-Historie
  verschlüsselt darin; Servernamen, Hostnamen, Benutzernamen, Filterregeln
  und Provider-Header standen im Klartext in der Datei. Jetzt ist die ganze
  Datei verschlüsselt, auch alles, was künftig dazukommt.
- Eine bestehende Datenbank wird beim ersten Start **automatisch
  umgewandelt**. Die Umwandlung wird vorher geprüft (Zeilenzahl je Tabelle,
  Integritätsprüfung, Migrationsstand); erst danach ersetzt sie die alte
  Datei. Scheitert ein Schritt, bleibt die bisherige Datenbank unverändert,
  und Smart SSH meldet das statt unverschlüsselt weiterzulaufen.

### Geändert

- Den Schlüssel der Datenbank leitet Smart SSH aus dem Schlüssel ab, der
  schon für den Chatverlauf im Schlüsselbund deines Betriebssystems liegt —
  es kommt **kein** zweiter Eintrag dazu.
- **Ohne diesen Schlüssel startet Smart SSH nicht mehr.** Bisher startete
  es bei einem gesperrten Schlüsselbund eingeschränkt weiter (ohne
  Chatverlauf). Da sich die verschlüsselte Datei ohne Schlüssel gar nicht
  öffnen lässt, fragt Smart SSH jetzt stattdessen: „Erneut versuchen" oder
  „Beenden". An deinen Daten wird dabei nichts verändert.
- Ist der Schlüssel verloren oder passt er nicht zur Datei, erzeugt Smart
  SSH **nie** stillschweigend einen neuen. Du bekommst eine klare Meldung
  und kannst bewusst neu anfangen; die bisherige Datei wird dabei
  umbenannt, nicht gelöscht, und der Dialog nennt ihren neuen Namen.

### Wichtig

- **Ältere Versionen von Smart SSH können die Datenbank nach dem Umstieg
  nicht mehr öffnen.** Sie melden dann „möglicherweise beschädigt … Backup
  einspielen" — **das stimmt nicht.** Die Datenbank ist in Ordnung, nur
  verschlüsselt. Spiel kein Backup ein, sondern nutze die neue Version.
- Ohne den Schlüssel sind die Daten nicht wiederherstellbar — auch nicht
  aus einem Backup, denn ein Backup braucht denselben Schlüssel.
- Die alte, unverschlüsselte Datei wird nach der Umwandlung entfernt. In
  Backups, Zeitmaschinen-Schnappschüssen oder auf dem Datenträger selbst
  können ältere, unverschlüsselte Fassungen trotzdem weiter liegen.
