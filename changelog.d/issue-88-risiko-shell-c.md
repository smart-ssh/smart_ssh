### Sicherheit
- Die Risiko-Anzeige bewertet jetzt auch das Kommando innerhalb von
  `bash -c '…'`/`sh -c '…'` (auch hinter `sudo` oder `env`): `bash -c
  'shutdown -h now'` zeigt Server-Risiko Rot statt „kein Risiko“, und die
  Bestätigung für rote Kommandos greift.
