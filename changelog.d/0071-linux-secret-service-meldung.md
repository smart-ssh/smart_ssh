<!-- ANNAHME A-1 (Fundstelle 5, s. ADR XXXX): Die unten genannten
     Paketnamen sind nicht gemessen und vor dem Release durch die manuellen
     Tests M1–M4 zu bestätigen. Diese Zeile entfernen, sobald das erledigt
     ist. -->

### Behoben
- Linux ohne Systemschlüsselbund: Statt des englischen Bibliothekstexts
  „No default store has been set, so cannot search or create entries" nennt
  Smart SSH jetzt den tatsächlichen Zustand, die dadurch blockierten
  Funktionen und das Paket, das ihn behebt — je nachdem, ob kein
  Secret-Service-Anbieter läuft (`gnome-keyring`, `kwalletd6`, KeePassXC),
  kein D-Bus-Session-Bus erreichbar ist (`dbus-user-session`) oder der
  Schlüsselbund nur gesperrt ist. Ein gesperrter Schlüsselbund führt dabei
  nie zu einem Installationsvorschlag.
- Der Startdialog behauptete bisher, ohne Schlüsselbund funktioniere alles
  außer dem Chat-Verlauf normal. Das stimmte nicht: Ohne Schlüsselbund
  lassen sich weder KI-Provider noch Server-Passwörter, Passphrasen oder
  Sudo-Passwörter speichern oder lesen. Der Dialog zählt jetzt auf, was
  wirklich blockiert ist — und was weiterhin geht (SSH-Agent, Schlüssel
  ohne Passphrase).
- Das Server-Formular meldete „kein Sudo-Passwort hinterlegt", wenn der
  Schlüsselbund gar nicht antworten konnte. Es zeigt jetzt einen neutralen
  Zustand, statt etwas zu behaupten, das es nicht wissen kann.

### Neu
- Die Diagnose-Ansicht zeigt eine Zeile „Systemschlüsselbund: verfügbar /
  nicht verfügbar (Grund)" samt nächstem Schritt — nachschlagbar auch dann,
  wenn der Startdialog bereits weggeklickt wurde.

### Geändert
- Die Startdialoge (Datenbank-, Host-Key- und Schlüsselbund-Fehler) sprechen
  jetzt Deutsch oder Englisch, abhängig von `LC_ALL`/`LC_MESSAGES`/`LANG`.
  Vorgabe bleibt Deutsch, wenn keine brauchbare Spracheinstellung gesetzt
  ist.
