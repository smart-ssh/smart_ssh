### Neu
- Beim Anlegen und Bearbeiten eines Servers steht neben Passwort, Private
  Key, SSH-Agent und Zertifikat jetzt eine fünfte Anmeldeart zur Wahl:
  „Schlüsseldatei“ — ein Pfad statt eines im Schlüsselbund gespeicherten
  Inhalts, wie `IdentityFile` in `ssh_config`. Vor dem Speichern zeigt die
  Oberfläche, ob die Datei existiert, ob die Rechte passen, ob sie wie ein
  OpenSSH-Schlüssel aussieht und ob sie verschlüsselt ist. Server-Liste und
  -Details zeigen den Pfad, an dem die Anmeldung hängt.
- Ein Knopf „In den Schlüsselbund übernehmen“ legt den Inhalt einer solchen
  Schlüsseldatei einmalig in den Systemschlüsselbund und stellt die
  Anmeldeart auf „Private Key“ um — die Ursprungsdatei bleibt dabei
  unverändert liegen. Vorher zeigt ein Dialog, welche Datei gelesen wird
  und was sich ändert.
