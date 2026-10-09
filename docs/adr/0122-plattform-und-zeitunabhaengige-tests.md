# ADR 0122 — Plattform- und zeitunabhängige Tests

## Status

Accepted

Übernimmt die bleibenden Regeln aus den früheren Specs 0089, 0091
(Testteil), 0093 und 0097. Diese Specs waren Aufgabenlisten, um die CI auf
Windows grün zu bekommen und wackelige Tests zu beheben; sie sind jetzt
Verweise (Issue #84). Das Verhalten der CI selbst steht in Spec 0090.

## Kontext

Die Tests laufen in der CI auf Linux, Windows und macOS (Spec 0090, A9).
Zwei Arten von Fehlschlägen haben die CI dort lange rot oder unzuverlässig
gemacht:

- Tests, die eine Unix-Eigenschaft voraussetzen (Symlinks, Rechtebits,
  Pfadform), und ein Test-Binary, das unter Windows gar nicht startete.
- Tests, die auf echte Zeit statt auf ein Ereignis warteten und unter Last
  scheiterten. „N-mal grün" belegte dabei nichts, weil sich lokal kein
  Fehlschlag erzwingen ließ.

## Entscheidung

Die folgenden Regeln gelten für neue und geänderte Tests. Code-Kommentare
verweisen auf sie mit „ADR 0122, Rn".

### Plattformen

- **R1 — Nur-Unix-Tests sind einzeln gekennzeichnet.** Ein Test, der eine
  Unix-Schnittstelle braucht (Symlinks, Rechtebits), läuft nur unter Unix.
  Die Bedingung steht am einzelnen Test oder Item, mit kurzem Grund im
  Kommentar. Kein `#[ignore]`, kein stilles Entfernen. Eine
  Lint-Unterdrückung ist nur an der einzelnen Stelle und nur für den
  Nicht-Unix-Zweig zulässig (`cfg_attr(not(unix), allow(...))`), nie auf
  Crate- oder Modulebene; `-D warnings` bleibt.
- **R2 — Abweichendes Plattformverhalten wird festgehalten, nicht
  übersprungen.** Verhält sich eine Plattform zulässig anders (etwa ein
  Verzeichnis als Schlüsseldatei: unter Windows ein Lesefehler statt
  „keine reguläre Datei"), prüft der Test die Erwartung je Plattform.
  Testpfade sind auf jeder Plattform absolut, wenn der Test einen
  absoluten Pfad meint. Der Produktcode wird dafür nicht angeglichen.
- **R3 — Pfade werden als Bestandteile verglichen**, nicht als
  Zeichenketten mit einem bestimmten Trenner.
- **R4 — Der SFTP-Testserver emuliert keine Rechte.** Er hält fest, welchen
  Modus der Client per `setstat` gesendet hat, und der Test prüft diese
  Übertragung auf allen Plattformen. Dass das Dateisystem den Modus danach
  meldet, prüft der Test zusätzlich nur unter Unix.
- **R5 — Das Unit-Test-Binary der App-Shell trägt unter Windows ein
  Manifest** mit der Abhängigkeit auf Common Controls v6; ohne startet es
  dort nicht, weil eine Abhängigkeit `TaskDialogIndirect` importiert, das es
  nur in dieser Version gibt. Die Wahl der
  Plattform liest das Ziel, nicht den Build-Rechner. Die beiden Wege für
  MSVC und GNU stehen in ADR 0085, Abschnitt 1.

### Zeit

- **R6 — Auf Ereignisse warten, nicht auf Dauer.** Ein Test wartet auf das
  Ereignis selbst (Signal, Kanal, Freigabe durch den Test, aufgelöstes
  Promise). Eine Obergrenze gegen Hängen ist erlaubt und großzügig
  (Richtwert 5 s); der gute Fall endet sofort.
- **R7 — Timer des Produktcodes steuert der Test** (Fake-Timer oder eine
  nur für Tests einstellbare Zeitkonstante, deren Produktivwert gleich
  bleibt). Das Ergebnis hängt nicht von der Dauer eines echten Intervalls
  ab.
- **R8 — „Blockiert nicht" und „endet früher" werden über Reihenfolge oder
  ein Signal bewiesen.** Ein Backend, das „noch nicht fertig" sein soll,
  wartet auf eine Freigabe durch den Test, die erst nach der zu prüfenden
  Antwort kommt — nicht auf eine feste Zeit. Ist ein Zeitvergleich
  unvermeidbar, liegen zwischen gutem und schlechtem Fall mindestens
  Faktor 20.
- **R9 — Frontend: eine gemeinsame Obergrenze.** Asynchrones Warten in den
  Frontend-Tests hat eine gemeinsame Obergrenze (5 s) statt Fristen je
  Aufruf. Die Frist je Test liegt deutlich darüber (15 s), damit ein
  Wackler als fehlgeschlagene Erwartung mit Inhalt erscheint und nicht als
  Zeitüberschreitung.
- **R10 — Laufzeitschranken gegen exponentielles Verhalten sind
  begründet.** Ein Test, der prüft, dass eine Prüfung auf bösartiger
  Eingabe schnell bleibt, nennt im Kommentar die gemessene Laufzeit und den
  Abstand zu einer exponentiellen Laufzeit. Die Schranke ist so gewählt,
  dass ein langsamer Runner sie nicht reißt, ein Rückfall aber schon.
- **R11 — Ports aus dem Betriebssystem können weitergegeben werden.** Ein
  Test, der einen gerade freigegebenen Port als „geschlossen" benutzt,
  rechnet damit, dass ein parallel gestarteter Testserver ihn bekommt: Er
  wiederholt mit frischem Port (höchstens fünfmal), sobald der Port
  unerwartet antwortet, und scheitert bei jedem anderen Fehler sofort.
- **R12 — Der Testserver bestätigt Schreiben erst nach dem Flush**, damit
  ein Test, der die Datei direkt danach von der Platte liest, sie
  vollständig sieht.

### Nachweis

Ein behobener Wackler wird mit einer **Verzögerungsprobe** belegt: eine
künstliche Verzögerung an der Stelle, auf die der Test wartet, lässt den
alten Test scheitern und den neuen grün; eine Gegenprobe mit kaputter
Implementierung lässt den neuen Test weiter scheitern. Wiederholte grüne
Läufe allein gelten nicht als Nachweis.

## Konsequenzen

- Kein Test wird unter Windows still abgeschaltet; was dort nicht gilt,
  ist am Test sichtbar.
- Testhilfen (Testserver, Mocks) bekommen gelegentlich Beobachtungspunkte
  (empfangener Modus, „erste Zeile gesendet", Freigaben), die es im
  Produktcode nicht gibt.
- Ein Rennen im Produktcode, das sich nur mit einem Eingriff dort beheben
  ließe, wird nicht im Test überdeckt, sondern als eigener Fehler
  behandelt.
