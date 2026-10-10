# ADR 0128 — Filter-Engine: Here-String-Inhalt einer Shell wird ausgewertet

Status: akzeptiert
Betrifft: Issue #53, Spec 0002 (Abschnitte 3, 4.4, 4.6), ADR 0001, ADR 0036, ADR 0107 (Entscheidung 7)

## Problem

Ein Kommando mit `<<` gilt nach ADR 0001 als undurchsichtiger Block und
endet bei `Confirm`. Für `bash -c "..."` extrahiert die Engine den Code und
wertet ihn rekursiv aus, sodass ein `Deny` dahinter greift. Für Here-Strings
gab es das nicht: `bash <<< "rm -rf /"` blieb auch mit einer expliziten
Regel `Deny "rm *"` bei `Confirm`. Spec 0002, Abschnitt 3 verlangt aber,
dass ein abgelehntes Kommando gar nicht erst zur Bestätigung angeboten
wird. ADR 0107 (Entscheidung 7) hatte das bewusst offen gelassen.

## Entscheidung

1. **Gleiche Behandlung wie `-c`.** Liest eine Shell ihr Programm aus einem
   Here-String, wird das Here-String-Wort entquotet (gleiche Regeln wie bei
   der `-c`-Extraktion) und als eigenes Kommando rekursiv ausgewertet. Das
   Ergebnis wird nur mit der bisherigen `Confirm`-Untergrenze zum
   strengeren kombiniert: `Confirm` bleibt das Minimum, ein `Deny` im Inhalt
   macht das ganze Kommando zu `Deny`. Kein neuer Decision-Code.

2. **Was als Shell zählt.** Das Ziel wird wie überall über Wrapper, `sudo`
   und Variablenzuweisungen aufgelöst. Es muss eine der Shells sein, die die
   Engine schon für `-c` und `... | sh` kennt (bash-, ksh-Familie, `sh`,
   `fish`, `csh`/`tcsh`, versionierte Namen) oder `source`/`.`, und es muss
   sein Programm laut Optionsanalyse von stdin lesen (also kein Skript-
   Operand, kein `-c`). Interpreter wie `python3` und unbekannte, nur
   „shell-artige“ Namen werden nicht extrahiert: ihr Inhalt ist kein
   Shell-Code, eine Auswertung als solcher könnte grundlos ablehnen.

3. **Nur Eindeutiges wird extrahiert.** Alles andere bleibt bei `Confirm`
   (oder `Deny`, wenn eine bestehende Prüfung das ohnehin ergibt):
   - Here-Docs (`<<`, `<<-`), mehrere Here-Strings und jede weitere
     Eingabe-Umleitung oder Process-Substitution mit `<` im Kommando — die
     Engine müsste sonst entscheiden, welche Quelle die Shell tatsächlich
     liest;
   - ein Here-String-Wort mit `$` oder Backtick oder mit führendem `~` —
     die Shell expandiert es, die Engine kann den Wert nicht kennen;
   - unausgeglichene Anführungszeichen;
   - ein leeres Here-String-Wort.

   Ein Kommando mit Verkettung (`echo hi; bash <<< '...'`) wird extrahiert,
   wenn genau ein Teilkommando den Here-String enthält.

4. **Tiefe.** Die Rekursion zählt wie der `-c`-Pfad gegen
   `MAX_SUBSTITUTION_DEPTH`; jenseits davon gilt ADR 0036 (`Deny`).
   Verschachtelte Here-Strings (`bash <<< "bash <<< '...'"`) werden Ebene für
   Ebene ausgewertet.

5. **Hard-Blacklist.** Ein Hard-Blacklist-Treffer im Here-String-Inhalt wird
   jetzt erkannt und im Trace gemeldet. Die Hard-Blacklist selbst ergibt
   laut Spec 0002, Abschnitt 3.1 weiterhin mindestens `Confirm`; an ihrer
   Wirkung ändert diese ADR nichts.

   Auslegung des Akzeptanzkriteriums 2 von Issue #53 („ohne Nutzerregeln
   ergibt ein Hard-Blacklist-Kommando hinter einem Here-String `Deny`“): Die
   Hard-Blacklist wirkt hinter einem Here-String genau wie anderswo. Ohne
   Nutzerregeln ergibt `bash <<< "rm -rf /"` also `Confirm` mit dem Code
   `FILTER_HARD_BLACKLIST`, genau wie `bash -c "rm -rf /"` und `rm -rf /`,
   und nie `AutoExec`. Greift zusätzlich eine `Deny`-Regel, gewinnt `Deny`.
   Ein `Deny` nur hinter Here-Strings wäre inkonsistent mit dem `-c`-Pfad;
   eine Hard-Blacklist, die überall `Deny` ergibt, wäre eine
   Produktentscheidung mit eigener Spec-Änderung und gehört nicht hierher.

## Konsequenzen

- `bash <<< "rm -rf /"` mit `Deny "rm *"` ist jetzt `Deny` statt `Confirm`.
  Kein Kommando wird dadurch lockerer bewertet; ein harmloser Here-String
  (`bash <<< "ls"`) bleibt `Confirm`, nie `AutoExec`.
- Bekannte Grenze: Steht hinter dem Here-String noch eine Ausgabe-
  Umleitung (`bash <<< "rm -rf /" 2>/dev/null`), erkennt die
  Optionsanalyse die Umleitung als Operand, der Inhalt wird nicht
  extrahiert, und das Kommando bleibt bei `Confirm`. Das ist fail-closed,
  aber kein `Deny`.
- Der Risiko-Klassifizierer bleibt unverändert; er sieht das Kommando wie
  bisher als einen Block.
