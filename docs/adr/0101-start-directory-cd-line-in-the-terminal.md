# ADR 0101 — Die `cd`-Zeile fürs Startverzeichnis im Terminal

Status: akzeptiert
Betrifft: Spec 0102 (§3, §4.2), Issue #9

## Problem

Issue #9 legt fest, dass das Terminal sichtbar per `cd -- '<dir>'` ins
Startverzeichnis wechselt, nachdem die Login-Shell gestartet ist, und dass
ein `'` im Pfad sicher maskiert wird. Offen blieben drei Punkte: wie `~/…`
gequotet wird (in einfachen Anführungszeichen expandiert keine Shell die
Tilde), mit welchem Byte die Zeile abgeschickt wird, und was mit Zeichen
geschieht, die eine eingetippte Zeile zerreißen.

## Entscheidung

1. **Absolute Pfade:** `cd -- '<pfad>'`, jedes `'` darin wird zu `'\''`.
   Alles zwischen den Anführungszeichen ist für die Shell Literal — kein
   `$()`, kein `;`, kein Glob.
2. **`~/…`:** Nur `~/` bleibt außerhalb der Anführung, der Rest ist wie oben
   gequotet: `cd -- ~/'my dir'`. `~/` allein wird `cd -- ~`. So expandiert
   die Login-Shell die Tilde selbst — auch dann richtig, wenn ein Profil die
   Shell vorher woandershin wechseln lässt.
3. **Abschluss mit `\r`**, dem Byte, das das Terminal für die Eingabetaste
   sendet. Zeileneditoren (readline, zle, fish) und der kanonische
   TTY-Modus vor dem ersten Prompt (ICRNL) nehmen es gleichermaßen an.
4. **Steuerzeichen sind im Wert verboten** (Validierung in Backend und
   Formular). Ein `\r` oder `\n` im Pfad würde die Zeile vorzeitig
   abschicken, ein Escape-Zeichen das Terminal steuern.
5. **Geschrieben wird nur bei gefundenem Verzeichnis** (Spec 0102, §4.1).
   Scheitert das Schreiben selbst, bleibt die Shell im Home; der Fehler wird
   mit Code protokolliert, die Sitzung läuft weiter.

## Abgewogene Alternativen

- **`~/…` vor dem `cd` per SFTP in einen absoluten Pfad auflösen:**
  `SftpSession` hat kein `realpath` (Spec 0020), und eine neue
  Trait-Methode wäre eine Schnittstellenänderung, die das Issue nicht
  vorsieht.
- **`"$HOME"/'…'`:** funktioniert ebenso, ist aber länger und weniger
  vertraut als `~/` in der sichtbaren Zeile.
- **`\n` statt `\r`:** in den gängigen Shells gleichwertig; `\r` entspricht
  genau dem Tastendruck.

## Konsequenzen

- Die Zeile ist im Terminal sichtbar und in der Shell-Historie, wie vom
  Issue gewollt.
- In fish gilt `\` auch innerhalb einfacher Anführungszeichen als Escape;
  ein Pfad mit Backslash kann dort abweichend interpretiert werden. In POSIX-
  Shells, bash und zsh tritt das nicht auf. Hingenommen — solche Pfade sind
  selten, und das Ergebnis ist ein sichtbar fehlschlagendes `cd`, kein
  stilles anderes Verhalten.
