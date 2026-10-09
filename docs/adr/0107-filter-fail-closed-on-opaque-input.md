# ADR 0107 — Filter-Engine: fail-closed bei undurchsichtiger Eingabe

Status: akzeptiert
Betrifft: Issue #13, Issue #55, Issue #60, Spec 0002 (Abschnitte 3, 4.4, 4.6), ADR 0001, ADR 0036

## Problem

Issue #13 verlangt eine adversariale Regressionssuite für die Filter-Engine
und, wo ein Fall durchrutscht, einen fail-closed Fix: „Eingabe, die sich
nicht eindeutig zerlegen lässt, gilt als gefährlich.“ Die Suite
(`crates/core/src/filter/adversarial_tests.rs`) prüft jeden Fall unter der
großzügigsten denkbaren Policy (`Allow "*"`, zusätzlich mit `Deny "rm *"`
und ohne Regeln). Gegen den Stand vor diesem Issue endeten 13 der 27 Tests
mit `AutoExec`, u. a. für `ｒｍ -rf /`, `r\u{200B}m -rf /`,
`$'\x72m' -rf /`, `eval "rm -rf /"`, `rm${IFS}-rf${IFS}/`,
`... | base64 -d | sh`, `SUDO rm -rf /`, `/usr/bin/env rm -rf /`,
`/bin/r? -rf /` und `(rm -rf /)`.

Offen ließ das Issue, *welche* Eingaben als „nicht eindeutig“ gelten und
wie weit die Engine sie auflösen soll.

## Entscheidung

1. **Nur Eskalation, nie Ersatz.** Jede neue Prüfung setzt über `combine`
   eine Untergrenze `Confirm`; die normale Auswertung läuft unverändert
   weiter, ein `Deny` gewinnt also weiterhin. Keine bestehende Prüfung
   wurde gelockert. Kein neuer Decision-Code: alle neuen Fälle melden
   `FILTER_PARSE_AMBIGUOUS` („konnte nicht sicher analysiert werden“) —
   genau das ist ihre Bedeutung, und so bleibt die Schnittstelle zum
   Frontend unverändert.

2. **Unicode und ANSI-C auf dem Gesamtkommando** (`opaque_encoding_reason`,
   einmal pro Auswertung): C1-Steuerzeichen, Leerraum außerhalb von ASCII
   (für `normalize_whitespace` ein Trenner, für die Shell Teil des Worts —
   die Engine würde ein anderes Kommando beurteilen als das ausgeführte)
   und unsichtbare Formatzeichen (Zero-Width, Bidi-Steuerung, BOM, Soft
   Hyphen, Variation Selectors, Tags). Außerdem jeder `$'...'`-String mit
   Backslash-Escape, weil die Engine diese Escapes nicht dekodiert.
   Sichtbare Nicht-ASCII-Zeichen in **Argumenten** (`echo "Grüße"`) bleiben
   erlaubt.

3. **Undurchsichtiger Kommandoname je Teilkommando**
   (`opaque_command_word_reason`): Der Kommandoname nach Wrapper-/sudo-/
   Zuweisungs-Entfernung und Entquotung muss aus ASCII-Buchstaben, Ziffern
   und `_ . - + : @ % , = / ~ ^` bestehen. Damit fallen Parameter-Expansion
   (`$x`, `${IFS}`), Glob/Brace (`/bin/r?`, `{rm,-rf,/}`), Fullwidth und
   Homoglyphen sowie Subshell-/Gruppen-/Negations-Syntax (`(rm`, `{`, `!`)
   auf `Confirm`. Shell-Schlüsselwörter (`if then else do while for case`
   …) ebenso: ein zusammengesetztes Konstrukt wird segmentweise nicht
   aufgelöst. Homoglyphen werden nicht über eine Confusables-Tabelle
   erkannt (keine neue Abhängigkeit), sondern über die einfachere,
   strengere Regel „Kommandoname nur ASCII“.

4. **Shell-, Interpreter- und `source`-Aufrufe mit Optionsparser**
   (`program_source`): Für POSIX-Shells (`sh bash rbash zsh dash ksh ksh93
   mksh lksh pdksh ash yash posh`), `fish`, `csh`/`tcsh`, `python*`,
   `perl`, `ruby`, `node` und `php` werden die Optionen je Programm
   ausgewertet (`OptionSpec`): Optionen mit Wert (`-o`/`+o`/`-O`,
   `--rcfile`, `python3 -W`, `perl -I`, `ruby -r`, `node --require` …)
   verbrauchen ihr Argument, `+`-Optionen gelten als Optionen. Ergebnis
   je Teilkommando:
   - **Code als Argument** (`-c` an beliebiger Stelle der Optionen, auch in
     einem späteren Kettenglied wie `ls; ksh -c "..."`; `fish --command`,
     `perl -e`, `php -r` …): Untergrenze `Confirm`, und der Code wird wie
     bei `eval` rekursiv ausgewertet, ein `Deny` greift also dahinter.
   - **Programm von der Standardeingabe** (kein Skript-Operand, `-s`, `-`,
     `/dev/stdin`, `/dev/fd/*`, `/proc/*/fd/*`; auch `source`/`.` mit so
     einem Pfad): `Confirm`.
   - **Nicht auswertbar** (unbekannte Langoption, nicht tokenisierbar,
     `-c` ohne Code): `Confirm`. Bei einer unbekannten Langoption weiß die
     Engine nicht, ob sie das nächste Wort verbraucht. „Nein“ zu raten
     würde einen Optionswert als Skript-Operand durchgehen lassen
     (`| bash -o errexit`), deshalb fail-closed. Wo unklar ist, ob eine
     Kurzoption einen Wert hat, gilt „ja“: ein Wort zu viel zu verbrauchen
     lässt höchstens den Operanden fehlen, und das eskaliert. Das gilt nur
     für ganze Wörter, nicht für den Rest eines Options-Clusters: Dort
     darf ein Wert keine spätere Code-Option verschlucken. Deshalb:
     - `-o`/`-O` (auch `+o`) verhalten sich je Shell verschieden:
       `bash`/`rbash`/`dash`/`ash` nehmen immer das **nächste Wort** als
       Wert, der Cluster wird weitergelesen (`bash -oc errexit CODE`
       führt `CODE` aus). `zsh` und die ksh-Familie (`ksh ksh93 mksh lksh
       pdksh`) nehmen den **Rest des Clusters**, wenn er nicht leer ist
       (`zsh -oerrexit -c CODE`), und nur ein alleinstehendes `-o` nimmt
       das nächste Wort. Ein **einbuchstabiger** `-o`-Wert (angehängt oder
       als nächstes Wort) ist in ksh93 die gleichnamige Kurzoption:
       `ksh -oc CODE` und `ksh -o c CODE` laufen wie `ksh -c CODE`. Für
       `zsh` und die ksh-Familie gilt deshalb `c` als Code- und `s` als
       Stdin-Option; ein negierter (`+o c`, `-o noc`), nur in
       Groß-/Kleinschreibung passender oder wertnehmender Buchstabe ist
       nicht auswertbar (`Confirm`). ksh93 ignoriert `-`/`_` in
       Optionsnamen (`-o c_`, `-o no-c` wirken wie `-c`), zsh zusätzlich
       Groß-/Kleinschreibung. Der Wert wird deshalb vor dem Lesen
       normalisiert (`-`/`_` entfernt, ein führendes `no` negiert). Statt
       jede weitere Schreibweise einzeln nachzutragen, gilt eine
       Positivliste: Ein mehrbuchstabiger Wert muss ein bekannter
       ksh-/mksh-/zsh-Optionsname sein, sonst ist der Aufruf nicht
       auswertbar (`Confirm`), auch ein leerer Wert. Die Namen, mit denen
       die Shell ihr Programm von stdin liest (zsh `SHIN_STDIN`, mksh
       `stdin`), zählen wie `-s`. Ein seltener, nicht gelisteter, aber
       harmloser Optionsname kostet damit nur eine Bestätigung. Für
       mksh/lksh/pdksh ist das Verhalten nicht nachgeprüft; sie bekommen
       dieselbe, nur verschärfende Behandlung. Bei `sh`, `yash` und `posh` ist nicht sicher,
       welche Shell dahinter steht; der Aufruf wird deshalb nach beiden
       Lesarten ausgewertet und zusammengeführt (fail-closed): findet eine
       der beiden Code, wird der Code beider Lesarten rekursiv
       ausgewertet; stimmen sie überein, gilt das gemeinsame Ergebnis;
       sonst ist der Aufruf nicht auswertbar (`Confirm`). Eine einheitliche
       Regel für alle POSIX-Shells hatte im Review die jeweils andere
       Familie fail-open gemacht.
     - Optionen mit reinem Ziffernwert (`perl -l`/`-0`, `ruby -0`/`-T`/`-W`)
       verbrauchen nur Ziffern, danach wird der Cluster weitergelesen
       (`perl -lne CODE`, `ruby -W2e CODE`). Ein Hex-Wert (`perl -0x…`)
       ist nicht auswertbar, weil Hex-Ziffern `e` enthalten.
     - Bei Optionen, deren angehängter Wert kein erkennbares Ende hat
       (`perl -M…`, `ruby -K…`), ist der Aufruf nicht auswertbar, sobald
       der Rest des Clusters ein Zeichen einer Code-, Stdin- oder
       Skript-Option enthält (`perl -Mlib=e x.pl` → `Confirm`).
     - Jeder verbrauchte Optionswert (kurz/lang, angehängt/getrennt), der
       die Standardeingabe benennt, macht den Aufruf zu „Programm von der
       Standardeingabe“ (`php -f /dev/stdin`, `node -r /dev/stdin app.js`).
   - **Sichtbarer Skript-Operand** (`bash -e deploy.sh`, `python3 -m
     http.server`, `source ~/.bashrc`): keine neue Untergrenze.
     Bewusst verändert gegenüber der ersten Fassung dieser Prüfung, die
     jedes Argument `-` als Standardeingabe wertete: `bash - x.sh` (ein
     einzelnes `-` beendet bei Shells die Optionen) und `bash x.sh -s`
     (`-s` nach dem Operanden ist ein Skript-Argument) zählen als
     sichtbarer Skript-Operand. Das entspricht dem Verhalten der Shell.

   Die Block-Erkennung für `bash -c` am Kommandoanfang (ADR 0001) wird
   dafür bewusst **nicht** erweitert. Würde etwa `ksh -c 'ls' && rm -rf /`
   als ein Block gelten, würde nur `ls` rekursiv geprüft und das bisher
   eigene Segment `rm -rf /` fiele von `Deny` auf `Confirm` zurück. Im
   Block-Pfad wird zusätzlich zum dritten Token auch der Code aus
   `program_source` ausgewertet (`sh -c -- CODE`), beide über `combine`.

   **Nachtrag Issue #60 — Shell-Namen außerhalb der Listen.** Vorher
   galt jeder nicht gelistete Name als gewöhnliches Kommando
   (`ls; oksh -c 'rm -rf /'` war unter `Allow "*"` `AutoExec`). Jetzt:
   - `rksh` (ksh93), `rzsh` (zsh) und `oksh` (OpenBSD-ksh, ein
     pdksh-Nachfahre) gehören zur ksh-Familie.
   - Versionierte Binaries bekannter Shells bekommen die Familie ihres
     Grundnamens: Grundname (POSIX-Shells, `fish`, `csh`, `tcsh`), optional
     `-` oder `_`, dann eine Version, die mit einer Ziffer beginnt und nur
     aus ASCII-Buchstaben, Ziffern, `.`, `+`, `-`, `_` besteht (`bash5`,
     `bash-5.2`, `zsh-5.9`, `ksh2020`, `ksh93u+m`). Die Ziffer direkt nach
     dem Namen hält `sha1sum`, `shred` oder `bashbug` heraus.
   - **Fail-closed-Auffangregel** für nicht klassifizierte Namen, die wie
     eine Shell aussehen: Basename endet auf `sh`, besteht nur aus
     ASCII-Buchstaben, Ziffern, `-`, `_` (also keine Skriptdatei wie
     `deploy.sh`) und steht nicht auf der Ausschlussliste `ssh autossh lsh
     chsh lchsh ypchsh flush fdflush crash push publish refresh rehash
     hash finish` (`ssh -c` ist eine Cipher, `chsh -s` setzt die
     Login-Shell). Trägt der Aufruf vor `--` einen kurzen Options-Cluster
     mit `c` oder `s` (`-c`, `-s`, `-xc`, `+s`), wird er wie `sh` nach
     beiden Lesarten ausgewertet: gefundener Code wird rekursiv geprüft
     (ein `Deny` greift also auch hinter `mysh -c`), sonst gilt er als
     Standardeingabe bzw. nicht auswertbar (`Confirm`). Ohne einen solchen
     Cluster bleibt alles wie vorher (`mysh deploy.sh`). Das kostet bei
     einem Nicht-Shell-Werkzeug mit passendem Namen gelegentlich eine
     Bestätigung, schließt aber die Klasse statt nur einzelner Namen.
   - Die Block-Erkennung `is_complex_shell_c_invocation` bleibt aus dem
     oben genannten Grund unverändert; alles läuft über `program_source`
     je Segment und eskaliert nur.

5. **`eval` wird wie `bash -c` behandelt:** Untergrenze `Confirm`, und der
   Code, den `eval` ausführt (Argumente entquotet, mit Leerzeichen
   verbunden), wird rekursiv als eigenes Kommando ausgewertet — ein
   `Deny "rm *"` greift also auch hinter `eval "rm -rf /"`. Die Rekursion
   zählt zur Tiefe aus ADR 0036.

6. **Wrapper case-insensitiv und per Basename**, zusätzlich `exec` und
   `builtin`; die `bash -c`-Erkennung ebenfalls case-insensitiv. Das
   betrifft nur `resolve_effective_command`, das ausschließlich für
   Deny-/Confirm-Regeln, die Hard-Blacklist und den Risiko-Klassifizierer
   (dort zusätzlich zum Rohtext) genutzt wird, nie für Allow-Regeln — mehr
   erkannte Wrapper können also nur eskalieren.

7. **Here-Strings bleiben bei `Confirm`.** `bash <<< "rm -rf /"` war schon
   vorher über die Here-Doc-Erkennung (ADR 0001) `Confirm`; der Inhalt wird
   nicht extrahiert, ein `Deny` dahinter greift also nicht. Das erfüllt
   „nie AutoExec“; eine Auflösung zu `Deny` wäre ein eigener Schritt.

   > **Nachtrag (Issue #53):** Abgelöst durch ADR 0122 — der Inhalt eines
   > Here-Strings, den eine Shell als Programm liest, wird jetzt wie `-c`-Code
   > ausgewertet; ein `Deny` dahinter greift.

8. **Hinterlegter Code: `trap` und `alias` (Issue #55).** Beide speichern
   Shell-Code, der *später* läuft — der `trap`-Handler bei einem Signal
   oder beim Beenden der Shell, der Alias-Wert bei jedem späteren
   Kommando, das mit dem Alias-Namen beginnt. Diese spätere Ausführung
   sieht die Engine nie (sie hat keinen Zustand über Kommandos hinweg),
   also muss die Definition selbst geprüft werden. Nach demselben
   Wrapper-Abschälen wie bei `eval` (`command`, `builtin`, `sudo`, ...)
   und case-insensitiv gilt:
   - **`trap` mit Handler** → Untergrenze `Confirm`; der Handler wird
     entquotet und rekursiv wie bei `eval` ausgewertet, ein `Deny` oder
     die Hard-Blacklist dahinter greift also. Ausgenommen sind nur Formen
     ohne Code: `trap`, `trap -p [SIG…]`, `trap -l` (Auflisten),
     `trap - SIG…` und `trap '' SIG…` (Zurücksetzen bzw. Ignorieren) sowie
     `trap SIG` mit genau einem Operanden, der wie ein Signalname aussieht
     (ASCII-Buchstaben, Ziffern, `_`, `+`, `-`; Bash setzt dann zurück).
     Ein einzelner Operand, der kein Signalname sein kann
     (`trap 'rm x'`), gilt als Handler. Unbekannte Optionen (`trap -z …`)
     und nicht zerlegbare Argumente → `Confirm` ohne Rekursion.
   - **`alias` mit mindestens einem Argument, das `=` enthält** →
     Untergrenze `Confirm`; jeder Wert wird rekursiv ausgewertet.
     Ausgenommen sind `alias`, `alias NAME…` und `alias -p` (Auflisten).
     Bewusst zählt *jedes* Argument mit `=`, auch hinter oder als
     Option (`alias -p x=y`, zsh `alias -g`), weil Shells sich darin
     unterscheiden, welche Optionen vor einer Definition stehen dürfen.
   - Der Alias-*Name* wird nicht ausgewertet und es gibt keinen Zustand
     über Kommandos hinweg: Das spätere `ls` nach `alias ls='rm -rf /'` ist
     für sich harmlos, geschützt wird an der Definition. Die Bestätigung
     dort ist der Punkt, an dem der Nutzer den hinterlegten Code sieht.
   - Funktionsdefinitionen (`f() { …; }`) deckt schon Punkt 3 ab (das `()`
     im Kommandowort).
   - Gemeldet wird wie bei `eval` `FILTER_PARSE_AMBIGUOUS` mit einem Grund,
     der `trap` bzw. `alias` nennt; trifft der Inhalt die Hard-Blacklist
     oder eine Regel, gewinnt deren strengerer Code.

## Konsequenzen

- Einige bisher unter einer breiten Allow-Regel automatisch ausgeführte,
  harmlose Formen verlangen jetzt eine Bestätigung: Schleifen und
  `if`-Konstrukte, `[ ... ]`, Kommandos mit Variablen als Kommandoname,
  `... | sh`, `eval`, `trap` mit Handler, `alias NAME=WERT`, `sh -c`/`python3 -c` in späteren Kettengliedern,
  Shell-/Interpreter-Aufrufe mit unbekannten Langoptionen oder ohne
  Operand (auch `python3 --version`), Eingaben mit unsichtbaren Unicode-Zeichen oder
  geschützten Leerzeichen. Das ist die vom Issue verlangte fail-closed
  Richtung.
- Der Risiko-Klassifizierer nutzt weiter `segment_command`; die neuen
  Prüfungen liegen bewusst in der Engine und nicht in `split_command`,
  damit dessen Segmentierung (und damit die Risiko-Einstufung) unverändert
  bleibt.
