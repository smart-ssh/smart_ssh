### Sicherheit
- Deny-Regeln greifen jetzt auch hinter `-c` von Shell-Varianten wie
  `oksh`, `rksh`, `rzsh` und versionierten Shells (`bash5`, `bash-5.2`,
  `zsh-5.9`, `ksh93u+m`). Unbekannte Programme, deren Name auf `sh` endet
  und die mit `-c`/`-s` aufgerufen werden (z. B. `pwsh -c …`), verlangen
  mindestens eine Bestätigung; `ssh`, `chsh` und Skriptdateien wie
  `deploy.sh` sind davon ausgenommen.
