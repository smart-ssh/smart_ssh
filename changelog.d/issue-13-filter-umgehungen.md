### Sicherheit
- Die Filter-Engine erkennt weitere Umgehungsversuche und verlangt dafür
  mindestens eine Bestätigung, auch wenn eine Allow-Regel greift:
  unsichtbare Unicode-Zeichen (Zero-Width, Bidi-Steuerung), geschützte und
  andere Unicode-Leerzeichen, C1-Steuerzeichen, Fullwidth- und
  Homoglyph-Kommandonamen, `$'\x..'`-Quoting, Variablen oder Glob-Muster
  als Kommandoname (`$x`, `${IFS}`, `/bin/r?`), Subshells und
  Shell-Konstrukte (`if`/`for`/`while`), Shells und Interpreter, die ihr
  Programm von der Standardeingabe lesen (`… | sh`, `… | bash -o errexit`,
  `source /dev/stdin`), und `eval`.
- Deny-Regeln greifen jetzt auch hinter `eval "…"`, hinter `-c` weiterer
  Shells (`ksh`, `mksh`, `ash`, `fish`, `csh`, `tcsh` …), hinter `-c` nach
  anderen Optionen (`bash -e -c "…"`) oder in einem späteren Kettenglied
  (`ls; bash -c "…"`), hinter Wrappern mit
  Pfad (`/usr/bin/env rm …`) oder in anderer Schreibweise (`SUDO`, `Env`)
  sowie hinter `exec`/`builtin`.
