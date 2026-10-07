### Sicherheit
- Die Filter-Engine erkennt weitere Umgehungsversuche und verlangt dafür
  mindestens eine Bestätigung, auch wenn eine Allow-Regel greift:
  unsichtbare Unicode-Zeichen (Zero-Width, Bidi-Steuerung), geschützte und
  andere Unicode-Leerzeichen, C1-Steuerzeichen, Fullwidth- und
  Homoglyph-Kommandonamen, `$'\x..'`-Quoting, Variablen oder Glob-Muster
  als Kommandoname (`$x`, `${IFS}`, `/bin/r?`), Subshells und
  Shell-Konstrukte (`if`/`for`/`while`), Shells, die ihr Programm von der
  Standardeingabe lesen (`… | sh`), und `eval`.
- Deny-Regeln greifen jetzt auch hinter `eval "…"`, hinter Wrappern mit
  Pfad (`/usr/bin/env rm …`) oder in anderer Schreibweise (`SUDO`, `Env`)
  sowie hinter `exec`/`builtin`.
