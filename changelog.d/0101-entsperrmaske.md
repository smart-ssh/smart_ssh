### Hinzugefügt

- **Entsperrmaske beim Start.** Ist ein Master-Passwort eingerichtet, fragt
  Smart SSH beim Start danach. Bis zur Entsperrung ist nichts anderes
  erreichbar: kein Server, keine Einstellung, kein MCP-Server.
- **Master-Passwort in den Einstellungen.** Eine neue Sektion zeigt, wo der
  Schlüssel zu deiner Datenbank liegt — im Schlüsselbund deines
  Betriebssystems oder in einer Schlüsseldatei, geschützt durch dein
  Master-Passwort. Dort lässt sich ein Master-Passwort einrichten, ändern
  und wieder abschaffen.
- **Ausweg bei vergessenem Passwort.** Nach drei vergeblichen Versuchen
  bietet die Entsperrmaske „Neu anfangen" an. Deine bisherigen Daten gehen
  dabei verloren — die Dateien werden nur umbenannt, nicht gelöscht, und du
  wirst noch einmal gefragt, bevor etwas passiert. **Ein vergessenes
  Master-Passwort lässt sich nicht wiederherstellen.**
- **Warnung vor dem Überschreiben eines fremden Schlüssels.** Liegt im
  Schlüsselbund schon ein anderer Schlüssel, fragt Smart SSH, bevor es ihn
  ersetzt: Backups, die mit diesem Schlüssel verschlüsselt sind, wären
  danach unlesbar.

### Geändert

- Lässt sich die Schlüsseldatei neben der Datenbank gerade nicht lesen —
  weil ein anderes Programm sie offen hält oder die Rechte nicht stimmen —,
  sagt Smart SSH das und ändert nichts. „Neu anfangen" wird in diesem Fall
  **nicht** angeboten: Über den Inhalt der Datei ist damit nichts gesagt.
