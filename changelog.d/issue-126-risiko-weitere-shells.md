### Sicherheit
- Die Risiko-Anzeige bewertet den Code hinter `-c` jetzt für jede Shell, die
  auch die Filter-Engine erkennt: `ksh -c 'reboot'`, `fish --command '…'`,
  `tcsh -c '…'`, versionierte Namen wie `bash5`/`zsh-5.9` und unbekannte
  `*sh`-Namen zeigen dasselbe Risiko wie das eingepackte Kommando, auch mit
  Optionen vor `-c` (`bash -e -c '…'`).
