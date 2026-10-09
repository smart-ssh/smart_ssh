# Spec 0060 — Glob `*` überquert `/` nicht (pfadförmige Muster)

Status: umgesetzt
Zweck: Eine Allow-Regel mit einem Pfad-Muster wie `cat /var/log/*` erlaubt nur Pfade in diesem Verzeichnis und lässt sich nicht durch `..` oder weitere Verzeichnisebenen aushebeln.
Bezüge: Spec 0002 (Filter-Engine, Rangfolge), Spec 0009 (Testen-Panel), Spec 0020 (Pfad-Normalisierung bei SFTP-Aktionen), Spec 0077 (ungültige Muster), ADR 0053.

Ohne diese Regel würde `cat /var/log/*` auch `cat /var/log/../../../etc/shadow`
treffen, weil das Glob-`*` über `/`-Grenzen hinweggeht. Eine harmlos
aussehende, legitim erteilte Allow-Regel gäbe dann Zugriff auf ganz andere
Pfade frei. Das ist eine Umgehung der Filter-Engine und wird entsprechend
streng behandelt.

## Die Design-Entscheidung

Ein pauschales Verbot, mit `*` über `/` zu gehen, würde den Normalfall
brechen: Kommando-Argument-Globs, die legitim Schrägstriche enthalten (etwa
über URL-artige Argumente), würden nicht mehr passen. Deshalb gibt es einen
**eigenen Glob-Modus für pfadförmige Muster**. Nur in diesem Modus überquert
`*` kein `/`. Alle anderen Globs verhalten sich wie bisher.

## 1. Erkennung pfadförmiger Muster

Ein Glob-Muster gilt als pfadförmig, wenn **mindestens ein Wort** (durch
Leerraum getrennt) ein `/` enthält und kein URL-Schema (`…://`) trägt. Das
umfasst:

- absolute Pfade (`/var/log/*`),
- ausdrücklich relative Pfade (`./foo/*`, `../foo/*`),
- bloße relative Pfade ohne `./` (`foo/bar/*`): Im Zweifel streng, denn ein
  zu strenges Muster passt im Zweifel auf weniger (der Nutzer muss dann
  bestätigen), nie auf mehr.

Ein Wort mit URL-Schema zählt nicht als Pfad: `curl http://example.com/*`
passt weiterhin über `/` hinweg. Enthält ein Muster mindestens ein Pfad-Wort,
gilt das **ganze** Muster als pfadförmig (der Glob lässt sich nicht
wortweise konfigurieren). Ein gemischtes Muster wie
`wget http://x/* -O /tmp/*` wird dadurch auch im URL-Teil strenger; das ist
ein seltener, bewusst hingenommener Fall.

## 2. Das Matching

- **Pfadförmiges Muster:** `*` passt auf **kein** `/`. `/var/log/*` passt auf
  `/var/log/foo`, aber nicht auf `/var/log/sub/foo` und nicht auf
  `/var/log/../etc/shadow`.
- **Nicht pfadförmig:** unverändert, `*` überquert `/`.
- **Erst normalisieren, dann vergleichen.** `.`- und `..`-Segmente sowie
  doppelte Schrägstriche in den Pfad-Wörtern des Kommandos werden rein
  lexikalisch aufgelöst (kein Dateisystemzugriff, keine Symlink-Auflösung),
  bevor das Muster geprüft wird. Das ist dieselbe Auflösung wie bei den
  SFTP-Aktionen (Spec 0020). Ein einzelnes `..` ohne eigenes `/`
  (`/var/log/..`) würde sonst trotz Verbots von `*` über `/` als ein Segment
  durchrutschen. Bei SFTP-Pseudokommandos ist der Pfad schon normalisiert,
  die zweite Normalisierung ändert dann nichts. Auch Wörter mit `://` im
  Kommando werden normalisiert (sonst ließe sich ein Pfad wie
  `/tmp/x://../../../etc/shadow` ausnutzen).
- **Shell-Metazeichen machen die Normalisierung unzuverlässig.** Enthält ein
  Pfad-Wort im Kommando Zeichen, die Quoting, Escaping oder Klammer-Expansion
  auslösen können (`\ ' " { } [ ] $` und Backtick), lässt sich nicht
  lexikalisch sagen, welcher Pfad daraus wird (`\..`, `".."`, `{..,..}`,
  `.[.]` ergeben nach der Shell-Expansion `..`). Dann gilt der strenge
  Vergleich als nicht erfüllt. Für Allow heißt das: kein Treffer, es bleibt
  bei `Confirm`, nie `AutoExec`.
- **Deny und Confirm werden nie schwächer.** Für diese Aktionen gilt
  zusätzlich der bisherige, großzügige Vergleich (`*` überquert `/`) als
  Alternative. Eine bestehende Regel `Deny: rm /home/u/*` greift also weiter
  auch auf `rm /home/u/sub/file`; sie wird durch die Normalisierung nur
  zusätzlich verstärkt.

## 3. Bestehende Regeln

Gespeicherte Regeln ändern sich nicht, nur ihre **Auswertung** wird
strenger: Eine vorhandene `Allow: cat /var/log/*` trifft ab dieser Regel
keine `../`-Ausbrüche mehr. Es gibt keinen Datenverlust und keine Migration.
Betroffen ist vor allem eine Regel, die mit einem einzelnen `*` mehrere
Verzeichnisebenen abdecken sollte: Ein einzelnes `*` deckt nur noch eine Ebene
ab. Für Deny- und Confirm-Regeln ist `**` der Ausweg (z. B. `/etc/**`); für
Allow-Regeln ist `**` kein pauschaler Rat (ADR 0053, Abschnitt 5). Das ist als
Verhaltensänderung im Changelog vermerkt.

## 4. Adversariale Fälle (Pflicht bei Änderungen)

Folgende Umgehungsversuche gegen eine Allow-Regel `cat /var/log/*` dürfen
nicht zu `AutoExec` führen:

- `/var/log/../../etc/shadow`, `/var/log/../log/../../etc/passwd`
- Quoting- und Escaping-Tricks (`\..`, `".."`, `'..'`, `{..,..}`, `.[.]`),
  Variablen und Backticks im Pfad
- doppelte Schrägstriche (`/var//log/*`) und `.`-Segmente (`/var/log/./x`)
- ein Pfad, der ein `://` enthält, ohne ein URL zu sein
- ein Muster, das fast pfadförmig aussieht, die Erkennung aber austricksen
  könnte
- relative Ausbrüche, soweit relative Pfade als pfadförmig gelten

## Sicherheitszusagen

- Eine Allow-Regel mit pfadförmigem Muster passt nie auf einen Pfad außerhalb
  ihres Verzeichnisses (kein `../`-Ausbruch, kein Überspringen von `/`).
- Im Zweifel strenger: weniger wird automatisch erlaubt.
- Nicht pfadförmige Globs bleiben unverändert.
- Es gibt keine doppelte oder widersprüchliche Behandlung neben der
  SFTP-Normalisierung.

## Testbarkeit

- `Allow: cat /var/log/*` passt auf `/var/log/syslog`, nicht auf
  `/var/log/../../etc/shadow` und nicht auf `/var/log/sub/deep`.
- Ein nicht pfadförmiger Glob (Argument ohne `/`) verhält sich wie bisher.
- Alle Fälle aus Abschnitt 4 führen zu „passt nicht" bzw. `Confirm`.
- Das Testen-Panel (Spec 0009) zeigt für ein `../`-Kommando kein „Allow"
  mehr, sondern „Confirm / keine Regel".

## Nicht Teil dieser Spec

- Die SFTP-Pfad-Normalisierung selbst (Spec 0020).
- Andere Glob-Fragen außerhalb von `*` über `/`.
