### Hinzugefügt

- **Master-Passwort als Alternative zum Schlüsselbund** (in Arbeit): Der
  Schlüssel zur verschlüsselten Datenbank kann künftig auch mit einem
  Master-Passwort verwahrt werden — gedacht für Linux-Systeme ohne Secret
  Service, wählbar aber für alle. Der Schlüssel selbst bleibt derselbe; das
  Passwort verschlüsselt nur ihn, in einer eigenen Datei neben der Datenbank
  (Argon2id, 64 MiB). Ein Wechsel in beide Richtungen und eine
  Passwortänderung tauschen nur diese Datei aus, die Datenbank bleibt
  unberührt.
- **Für ein vergessenes Master-Passwort gibt es keine Wiederherstellung.**
  Ohne das Passwort sind Server, Zugangsdaten, Regeln und Verläufe endgültig
  verloren. Beim Einrichten wird darauf hingewiesen und eine Bestätigung
  verlangt.

Die Oberfläche dazu (Entsperrmaske und Einstellungen) folgt im nächsten
Schritt; bis dahin ist der Modus nicht einschaltbar und am
Schlüsselbund-Betrieb ändert sich nichts.
