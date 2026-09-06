# Spec: Ressourcen-Caps gegen feindliche/fehlerhafte Server

Status: Entwurf
Modul: `crates/ssh-transport` (Empfangsschleife), `crates/core/src/filter`
(Rekursions-Cap), `crates/core/src/ai` bzw. Zweitmeinungs-Pfad
(Längen-Cap)
Abhängigkeiten: SSH-Transport (0005), Filter-Engine (0002), Risiko-
Indikatoren/Zweitmeinung (0026), Fencing (0039)

> **Nummerierung**: nächste freie Nummer in deiner Reihe. Behebt drei
> Backlog-Funde mit gemeinsamem Muster.

## 1. Gemeinsames Muster

Drei unabhängig gefundene Stellen haben dieselbe Schwäche: Eine Grenze
greift erst **nach** vollständiger Verarbeitung statt **während** — ein
feindlicher oder fehlerhafter Server (oder eine bösartige KI-Ausgabe) kann
vorher Ressourcen erschöpfen. In allen drei Fällen ist die Richtung
fail-safe (nichts wird unsicher, es wird nur zu spät begrenzt), aber die
Ressourcenerschöpfung selbst ist real. Diese Spec zieht die Grenzen jeweils
an die richtige Stelle vor.

Leitprinzip: **Begrenzen während des Konsumierens, nicht danach.** Bei
Überschreitung wird sauber abgebrochen mit einer klaren, im UI sichtbaren
Meldung — nie ein Absturz, nie ein OOM, nie ein Hänger.

## 2. Fund A — Output-Cap greift erst nach vollständigem Puffern

### Problem
Der 2-MB-Output-Cap für Kommando-Ausgaben wird aktuell erst angewendet,
**nachdem** die gesamte Ausgabe gepuffert wurde. Ein hostiler Server, der
z. B. `yes AAAA | head -c 5G` liefert (adversarial getestet), kann den
Speicher erschöpfen, **bevor** der Cap greift — der Cap schützt nur, was an
KI/Log geht, nicht den Prozessspeicher.

### Fix
Refactoring der Empfangsschleife in `ssh-transport`: Bytes werden
**während** des Streamings gezählt, und sobald das konfigurierbare Limit
(Default 2 MB) erreicht ist, wird das Lesen **abgebrochen** — der Rest der
Serverausgabe wird verworfen, der Channel geschlossen bzw. das weitere
Lesen gestoppt. Der Puffer wächst nie über das Limit hinaus.

- Das Ergebnis wird als "abgeschnitten" markiert (`truncated: true` o. Ä.
  im `CommandOutput`), damit UI und KI-Kontext das kenntlich machen können
  ("Ausgabe bei 2 MB abgeschnitten").
- Gilt für den Exec-Modus (`execute`) — der interaktive PTY-Modus streamt
  ohnehin fortlaufend ins Terminal und puffert nicht auf dieselbe Weise;
  falls dort ein analoges Problem besteht (unbegrenzter Scrollback-Puffer),
  in dieser Spec **prüfen und melden**, aber nur fixen, falls es dieselbe
  Erschöpfungsklasse ist.
- Das Limit gilt **pro Kommandoausführung**, nicht kumulativ über eine
  Session.

## 3. Fund B — Kein expliziter Rekursions-Cap für verschachtelte Command-Substitution

### Problem
Spec 0002, Abschnitt 4.7 fordert einen Rekursions-Cap für verschachtelte
`$(...)`/Backticks explizit; aktuell ist er nur **indirekt** über den
Längen-Cap gedeckt. Tief verschachteltes `$(echo $(echo $(...)))`
**innerhalb** der Längengrenze könnte theoretisch tief genug rekursieren,
um beim Parsen einen Stack-Overflow auszulösen (der Filter-Engine-Parser
und der Risiko-Klassifizierer steigen beide in die Substitution ab). Der
Reviewer stufte den Schweregrad als unsicher (kein bestätigter Exploit)
ein — aber die Grenze fehlt tatsächlich explizit.

### Fix
Expliziter Tiefen-Cap beim Parsen verschachtelter Command-Substitution
(Default z. B. 32 Ebenen — großzügig genug für jeden legitimen Fall, weit
unter jeder Stack-Overflow-Schwelle). Bei Überschreitung: Das Kommando wird
**blockiert** (`Deny`) mit klarem Grund ("Kommando zu tief verschachtelt,
konnte nicht sicher analysiert werden") — **nie** ein unbegrenzter
rekursiver Abstieg, **nie** `AutoExec`.

> **Korrektur (aus dem 0043-Review)**: Eine frühere Fassung dieser Spec
> schrieb `Confirm` statt `Deny`. Das war ein **Sicherheitsfehler**: Ein
> gezielt über den Cap hinaus verschachteltes Kommando (z. B. 33 Ebenen
> `echo $(...)` um ein `rm -rf /`, ~270 Byte, unter dem Längen-Cap) hätte
> eine `Deny "rm *"`-Regel **umgangen** — der Cap stoppt die Rekursion,
> *bevor* der Parser das innere `rm` erreicht, und ein bloßes `Confirm`
> hätte dem Nutzer ein zahmes "bitte bestätigen" gezeigt, ohne dass die
> Deny-Regel je greift. Ein bewusst analyse-vereitelndes Kommando ist
> verdächtig, nicht harmlos — es wird blockiert, konsistent mit dem
> `Empty`-Command-Präzedenzfall. Dokumentiert in ADR 0036 mit
> Regressionstest, der den exakten Bypass reproduziert.

- Gilt für **beide** Konsumenten der Parselogik einheitlich: Filter-Engine
  (0002) **und** Risiko-Klassifizierer (0026) — konsistent damit, dass beide
  bereits dieselbe Normalisierung/denselben Längen-Cap teilen (Audit-
  Korrekturrunde). Kein Konsument steigt tiefer ab als der andere.
- Der Cap wird als iterativer oder tiefenbegrenzter Abstieg umgesetzt, nicht
  als "erst komplett parsen, dann Tiefe prüfen" (das hätte dasselbe
  Zu-spät-Problem wie Fund A).

## 4. Fund C — Kein Längen-Cap für den an den Zweitmeinungs-Provider gesendeten Inhalt

### Problem
Der an den optionalen Zweitmeinungs-Provider (Risiko-Zweitmeinung Spec 0026,
KI-Fencing-Prüfung Spec 0039) gesendete Inhalt hat keinen Längen-Cap. Reine
Kosten-/Timeout-Frage (kein Secret-Leak, Redaction läuft davor), aber ein
sehr großer Inhalt könnte unnötige Kosten/Timeouts verursachen.

### Fix
Konfigurierbarer Längen-Cap für den Inhalt, der an den Zweitmeinungs-
Provider geht (Default z. B. 16 KB — Zweitmeinung braucht nur genug Kontext
zur Einschätzung, nicht den vollen Output). Bei Überschreitung wird der
Inhalt **vor dem Senden** gekürzt (nicht die eigentliche Ausführung
betroffen, nur die Zweitmeinung), mit einem Hinweis an den Zweitmeinungs-
Prompt, dass gekürzt wurde. Da die Zweitmeinung nur eskalieren kann (nie
abschwächen, Spec 0026/0039), ist eine gekürzte Eingabe fail-safe: Im
schlimmsten Fall übersieht die Zweitmeinung etwas, das im abgeschnittenen
Teil lag — das regelbasierte Ergebnis bleibt davon unberührt.

## 5. Sicherheits-Invarianten

- Keiner der drei Caps darf eine Sicherheitsentscheidung **abschwächen**:
  Fund A markiert nur als abgeschnitten, Fund B **blockiert** (`Deny`) ein
  über den Cap hinaus verschachteltes Kommando (nie `AutoExec`, nie ein
  zahmes `Confirm`, das eine Deny-Regel umgehen ließe), Fund C betrifft nur
  die (rein eskalierende) Zweitmeinung.
- Der Rekursions-Cap (Fund B) gilt für Filter-Engine und Risiko-
  Klassifizierer **identisch** — kein Konsument hat eine tiefere/schwächere
  Sicht.
- Kein Cap führt zu einem Absturz/Panic/Hänger bei Überschreitung — immer
  sauberer, sichtbarer Abbruch.

## 6. Testbarkeit

- Fund A: Ein Mock-/Test-Server, der mehr als das Limit liefert (analog zum
  adversarialen `yes | head -c 5G`, aber im Test mit kleinerem Limit),
  belegt, dass der Puffer nie über das Limit wächst und das Ergebnis als
  abgeschnitten markiert ist. (Hängt ggf. an der connect-Mockbarkeit — falls
  nicht sauber testbar, gegen den in-process-Testserver aus 0005 lösen.)
- Fund B: Ein Kommando mit Verschachtelung **über** dem Cap landet bei
  `Confirm` mit dem korrekten Grund, ohne Stack-Overflow — je ein Test für
  Filter-Engine und Risiko-Klassifizierer. Ein Kommando knapp **unter** dem
  Cap wird normal geparst (kein falscher Alarm).
- Fund C: Inhalt über dem Cap wird vor dem Zweitmeinungs-Aufruf gekürzt;
  das regelbasierte Ergebnis bleibt unverändert; die Zweitmeinung kann
  weiterhin nur eskalieren.

## 7. Offene Punkte

- Exakte Default-Grenzwerte (2 MB / 32 Ebenen / 16 KB) sind Vorschläge —
  falls Telemetrie/Erfahrung später andere Werte nahelegt, konfigurierbar
  halten, aber nicht als Nutzer-Bedienknopf (interne Konstanten mit klarer
  Benennung reichen).
- PTY-Scrollback-Begrenzung (aus Fund A, falls dort dieselbe Klasse
  vorliegt) ggf. als eigener kleiner Nachzieher, nicht zwingend hier.
