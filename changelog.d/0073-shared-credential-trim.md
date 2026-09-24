### Behoben
- „Verbindung testen" bewertet leere und eingefügte Zugangsdaten jetzt
  genau wie „Speichern". Legt man einen neuen Server an und lässt das
  Passwort-, Schlüssel- oder Zertifikatsfeld leer, meldet der Test die
  Fehlermeldung, statt sich mit einem leeren Wert anzumelden — auf einem
  Server, der leere Passwörter erlaubt, konnte er dafür bisher Erfolg
  melden, obwohl sich derselbe Server anschließend nicht speichern ließ.
  Beim Bearbeiten bedeutet ein leeres Feld weiterhin „das hinterlegte
  Zugangsdatum verwenden".
  Die Meldung ist dieselbe (und in der eingestellten Sprache) wie beim
  Speichern, etwa „Passwort ist erforderlich".
- „Verbindung testen" behandelt die Passphrase einer Schlüsseldatei oder
  eines privaten Schlüssels jetzt genauso wie „Speichern". Bisher konnte
  derselbe eingefügte Wert den Verbindungstest scheitern lassen und
  danach trotzdem richtig gespeichert werden — der Test sagte damit
  etwas anderes aus als der Server, der daraus entstand.
- Ein eingefügter API-Key oder ein eingefügtes Passwort funktioniert jetzt
  auch dann, wenn beim Kopieren ein unsichtbares Zeichen an den Rand
  geraten ist — etwa ein BOM aus einer Textdatei oder ein Zero-Width-Space
  aus einer Webseite. Bisher wurde ein solcher Wert unverändert
  gespeichert, die Anmeldung schlug fehl, und der Key sah in der
  Oberfläche trotzdem richtig aus.

### Geändert
- API-Key, Server-Passwort, Passphrase und Sudo-Passwort werden an den
  Rändern nach derselben Regel bereinigt. Nur die Ränder: Was innerhalb
  eines Wertes steht, bleibt unangetastet — auch ein ungewöhnliches
  Zeichen, das dort hingehören könnte.
- Beim selbst gesetzten Pfad zum `sftp-server` (erhöhter Dateibrowser)
  gilt dieselbe Regel: Ein unsichtbares Zeichen am Rand wird entfernt,
  statt den Pfad als ungültig abzulehnen. Ein solches Zeichen
  **innerhalb** des Pfades führt unverändert zur Ablehnung.
