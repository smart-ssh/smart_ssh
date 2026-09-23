### Behoben
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
