### Behoben
- Linux ohne Systemschlüsselbund: Statt des englischen Bibliothekstexts
  „No default store has been set, so cannot search or create entries" nennt
  Smart SSH jetzt den tatsächlichen Zustand, die dadurch blockierten
  Funktionen und das Paket, das ihn behebt — je nachdem, ob kein
  Secret-Service-Anbieter läuft (`sudo apt install gnome-keyring`, auch
  unter KDE), kein D-Bus-Session-Bus erreichbar ist
  (`sudo apt install dbus-user-session`) oder der Schlüsselbund nur
  gesperrt ist. Ein gesperrter Schlüsselbund führt dabei nie zu einem
  Installationsvorschlag. Wer KWallet oder KeePassXC ohnehin nutzt, wird
  darauf hingewiesen, dass deren Secret-Service-Integration laufen bzw.
  eingeschaltet sein muss — Nachinstallieren allein genügt dort nicht.
- Der Startdialog behauptete bisher, ohne Schlüsselbund funktioniere alles
  außer dem Chat-Verlauf normal. Das stimmte nicht: Ohne Schlüsselbund
  lassen sich weder KI-Provider noch Server-Passwörter, Passphrasen oder
  Sudo-Passwörter speichern oder lesen. Der Dialog zählt jetzt auf, was
  wirklich blockiert ist — und was weiterhin geht (SSH-Agent, Schlüssel
  ohne Passphrase).
- Das Server-Formular meldete „kein Sudo-Passwort hinterlegt", wenn der
  Schlüsselbund gar nicht antworten konnte. Es zeigt jetzt einen neutralen
  Zustand, statt etwas zu behaupten, das es nicht wissen kann.
- „Hinterlegtes Sudo-Passwort entfernen" meldete Erfolg, auch wenn der
  Schlüsselbund den Eintrag gar nicht löschen konnte — das Passwort wäre
  beim nächsten `sudo` weiter eingespeist worden. Der Vorgang schlägt jetzt
  sichtbar fehl.
- Einen Server zu löschen funktioniert weiterhin auch dann, wenn der
  Schlüsselbund klemmt. Neu ist: Smart SSH sagt danach ausdrücklich, welche
  Einträge im Schlüsselbund zurückgeblieben sind, dass sie dort jetzt
  verwaist sind und wie sie sich von Hand entfernen lassen.

### Neu
- Die Diagnose-Ansicht zeigt eine Zeile „Systemschlüsselbund: verfügbar /
  nicht verfügbar (Grund)" samt nächstem Schritt — nachschlagbar auch dann,
  wenn der Startdialog bereits weggeklickt wurde.

### Geändert
- Die Startdialoge (Datenbank-, Host-Key- und Schlüsselbund-Fehler) sprechen
  jetzt Deutsch oder Englisch, abhängig von `LC_ALL`/`LC_MESSAGES`/`LANG`.
  Vorgabe bleibt Deutsch, wenn keine brauchbare Spracheinstellung gesetzt
  ist.
