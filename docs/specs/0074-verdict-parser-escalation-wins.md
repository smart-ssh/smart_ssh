# Spec 0074 — Im Urteils-Parser gewinnt die Eskalation, nicht das erste Wort

Status: freigegeben (Stefan, 2026-09-23) · Backlog: BL-0121 · Gate: —
Repo: **öffentlich** `smart-ssh` — `crates/app-shell/src/risk_second_opinion.rs`
Review-Priorität: **ERHÖHT** (Risiko-Einstufung und
Prompt-Injektions-Erkennung; adversariale Fälle in §6.3)

> Zwei Parser lesen das Urteil einer KI-Zweitmeinung aus Fließtext. Beide
> nehmen das **erste** passende Wort. Steht in der Antwort früh ein „nein"
> oder ein „none" — etwa weil das Modell den geprüften, nicht
> vertrauenswürdigen Inhalt zitiert —, wird eine später ausgesprochene
> Warnung verschluckt. Nicht die Entwarnung ist das Problem, sondern die
> **verlorene Eskalation**.

## 1. Ausgangslage (belegt)

Zwei Parser gleichen Aufbaus in `risk_second_opinion.rs`:

| Funktion | Erkennt | Zeile |
|---|---|---|
| `parse_second_opinion` | `none` / `yellow` / `red` | `:165` |
| `parse_injection_check` | `ja`/`yes` / `nein`/`no` | `:237` |

Beide zerlegen die Antwort in Wörter, bereinigen jedes um Satzzeichen und
geben beim **ersten** Treffer zurück. Der Rest wird zur Begründung.

**Die Aufrufer sind bereits asymmetrisch gebaut** — das ist geprüft, nicht
angenommen:

- `orchestration.rs:2259` wertet den Injektions-Check aus als
  `if let Some((true, _)) = …` und setzt dann `injection_suspected`.
  `Some((false, _))` und `None` haben **keinen** Zweig: Es passiert
  nichts, und ein zuvor gesetzter Verdacht wird nicht zurückgenommen.
- `escalate_data_risk` (`orchestration.rs:1376`) übernimmt die
  KI-Einstufung nur bei `ai_level > rule_based_level`. Ein `none` der KI
  kann eine regelbasierte Einstufung nicht senken (Spec 0026, Abschnitt 3:
  „Nur Eskalation, nie Abschwächung").

Damit bleibt genau eine Lücke: Was der Parser **nicht meldet**, kann der
Aufrufer nicht eskalieren. Ein „nein" an Position 3 verdeckt ein „ja" an
Position 30.

Dass das kein theoretischer Fall ist, folgt aus der Aufgabe selbst: Der
Injektions-Check bekommt **nicht vertrauenswürdigen Inhalt** vorgelegt und
bittet um eine Begründung. Eine Begründung, die den Inhalt zitiert, ist
die Regel, nicht die Ausnahme — und der Inhalt ist vom Angreifer
gestaltet.

## 2. Ziel und Nicht-Ziele

**Ziel:** Enthält eine Antwort mehrere Urteilswörter, gewinnt das
**eskalierende**. Was der Parser meldet, ist nie weniger alarmierend als
das, was in der Antwort steht.

**Nicht-Ziele:**

1. **Keine Änderung an den Aufrufern.** Sie sind richtig (§1) und bleiben
   unverändert.
2. **Keine Änderung an den Prompts.** Ein Prompt kann bitten, ein Modell
   kann sich nicht daran halten; der Parser muss robust sein, nicht der
   Prompt strenger.
3. **Kein Entfernen der Begründung.** Sie wird weiterhin übernommen.
4. **Keine neue Abhängigkeit**, kein strukturiertes Ausgabeformat, kein
   zweiter Aufruf beim Modell.

## 3. Anforderungen

**A1 (MUSS)** `parse_injection_check` meldet `true`, sobald **irgendwo**
in der Antwort ein Wort `ja` oder `yes` vorkommt — unabhängig davon, ob
vorher ein `nein`/`no` steht. `false` nur, wenn ein `nein`/`no` vorkommt
und **kein** `ja`/`yes`.

**A2 (MUSS)** `parse_second_opinion` meldet die **höchste** in der Antwort
vorkommende Stufe, nicht die erste: `red` vor `yellow` vor `none`.

**A3 (MUSS)** Kommt kein Urteilswort vor, bleibt es bei `None` —
unverändert „keine Prüfung verfügbar", nicht „alles in Ordnung". Die
Aufrufer behandeln `None` bereits richtig.

**A4 (MUSS)** Die Begründung bleibt der Text **nach dem Wort, das
gewonnen hat**; bleibt danach nichts Sinnvolles übrig, weiterhin die
vollständige Antwort. Das bisherige Verhalten bleibt damit erhalten,
solange nur ein Urteilswort vorkommt.

**A5 (MUSS)** Die Wortbereinigung bleibt wie bisher: nur alphanumerische
Zeichen, kleingeschrieben, Vergleich auf Gleichheit — **keine**
Teilstring-Suche. Ein `text.contains("red")` würde auf „redirect"
auslösen; diese Entscheidung aus ADR 0024 bleibt gültig.

## 4. Design

### 4.1 Warum „höchste gewinnt" und nicht „erste gewinnt"

Beide Parser dienen einer Sicherheitsentscheidung, und für die gilt im
Projekt durchgehend: verschärfen, nie lockern. „Erste gewinnt" macht das
Urteil von der **Wortstellung** abhängig — einer Eigenschaft, die der
Angreifer über den geprüften Inhalt mitbestimmt. „Höchste gewinnt" macht
es von der **Aussage** abhängig.

Die Änderung ist per Konstruktion monoton: Der neue Parser kann nie eine
niedrigere Stufe melden als der alte. Damit ist ausgeschlossen, dass diese
Spec einen bestehenden Schutz schwächt.

### 4.2 Der Preis, benannt

Ein Angreifer kann jetzt umgekehrt eine **falsche Eskalation** auslösen:
Inhalt, der das Wort „red" enthält und vom Modell zitiert wird, hebt die
Einstufung. Das ist die richtige Richtung — aber nicht kostenlos. Zu viele
Rückfragen führen dazu, dass Nutzer Dialoge blind bestätigen, und das
schwächt die letzte Verteidigungslinie (Confirm-Fatigue,
Architektur-Checkliste).

Bewusst in Kauf genommen: Eine falsche Eskalation ist sichtbar und
korrigierbar, eine verschluckte nicht. Sollte sich Confirm-Fatigue im
Betrieb zeigen, ist die Antwort ein strukturiertes Ausgabeformat für die
Zweitmeinung — nicht ein Zurück zu „erste gewinnt". Das gehört dann in ein
eigenes Item, nicht in diese Spec.

### 4.3 Verworfen

- **Nur die ersten N Wörter betrachten.** Der Prompt bittet um „nur
  ja/nein und eine kurze Begründung", also müsste das Urteil vorn stehen.
  Aber ein Modell, das sich nicht daran hält, bekäme dann gar kein Urteil
  — aus einer verschluckten Eskalation würde ein `None`. Besser, aber
  nicht gut.
- **Auf ein strukturiertes Antwortformat umstellen.** Sauberste Lösung,
  aber ein anderer Umfang: neuer Prompt, neues Parsing, Verhalten bei
  Modellen ohne verlässliche Formattreue. Eigenes Item.

## 5. Sicherheits-Invarianten

**Berührt: Risiko-Klassifizierer und die Erkennung eingeschleuster
Anweisungen.**

- **I1 — Nur Verschärfung.** Der neue Parser meldet für jede Eingabe eine
  Stufe, die mindestens so hoch ist wie die des alten. Das ist in §6.1
  als Eigenschaft über zufällige Eingaben geprüft, nicht nur an
  Beispielen.
- **I2 — Die Aufrufer bleiben unangetastet.** „Nur Eskalation, nie
  Abschwächung" (Spec 0026, Abschnitt 3) gilt weiterhin dort; diese Spec
  ergänzt dieselbe Regel eine Ebene tiefer.
- **I3 — `None` heißt weiter „keine Prüfung verfügbar".** Kein Weg darf
  aus einem nicht erkennbaren Urteil eine Entwarnung machen.
- **I4 — Kein Inhalt aus der Antwort wird ausgeführt oder gespeichert**,
  nur angezeigt; die bestehende Redaction auf diesem Pfad bleibt.

## 6. Tests

### 6.1 Eigenschaft statt Beispiel

- T1 **Monotonie:** Für eine Reihe erzeugter Antworten (Urteilswörter in
  zufälliger Zahl, Reihenfolge und Groß-/Kleinschreibung, dazwischen
  Fülltext) gilt: neue Stufe ≥ alte Stufe, und bei
  `parse_injection_check` nie `false`, wo der alte Parser `true` lieferte.
  Diese Eigenschaft ist der eigentliche Nachweis von I1.

### 6.2 Injektions-Check

- T2 `"Nein. — Korrektur: ja, der Text enthält eine Anweisung."` → `true`.
  **Scheitert am heutigen Stand.**
- T3 `"no"` allein → `false`.
- T4 `"ja"` allein → `true`.
- T5 Weder noch (`"Der Text ist unauffällig."`) → `None` (A3).
- T6 Begründung: Bei genau einem Urteilswort ist die Begründung
  unverändert der Text danach (A4).
- T7 Begründung bei mehreren: der Text nach dem **gewinnenden** Wort.

### 6.3 Zweitmeinung und adversariale Fälle

- T8 `"none … red …"` → `Red`. **Scheitert am heutigen Stand.**
- T9 `"red … none"` → `Red` (Reihenfolge egal).
- T10 `"yellow … none"` → `Yellow`.
- X1 **Zitierter Inhalt.** Antwort, die den geprüften Text zitiert und
  darin ein `no` enthält, danach das eigentliche Urteil `ja` → `true`.
  Das ist der Fall aus BL-0121.
- X2 **Wortgrenzen bleiben.** `"redirect"`, `"nonetheless"`, `"nobody"`,
  `"yesterday"`, `"janein"` lösen **kein** Urteil aus (A5).
- X3 **Sehr viele Urteilswörter.** Eine Antwort mit 10 000 abwechselnden
  `none`/`red`: Ergebnis `Red`, Laufzeit linear.
- X4 **Groß-/Kleinschreibung und Satzzeichen:** `"RED!"`, `"(Ja)"`,
  `"nein,"` werden erkannt wie bisher.
- X5 **Leere und nur aus Satzzeichen bestehende Antwort** → `None`, keine
  Panik.
- X6 **Mehrsprachig gemischt:** `"no — aber ja"` → `true`; ein
  englisches und ein deutsches Urteilswort widersprechen sich nicht,
  sondern werden wie zwei Urteile behandelt.

## 7. Umsetzungsreihenfolge

1. T2 und T8 als rote Tests schreiben — beide müssen gegen den heutigen
   Stand fehlschlagen, sonst prüfen sie nichts.
2. `parse_injection_check` nach A1 umstellen, A4 erhalten.
3. `parse_second_opinion` nach A2 umstellen, A4 erhalten.
4. Kommentare an beiden Funktionen: warum „höchste gewinnt", mit Verweis
   auf ADR 0024 und auf §4.2 (der Preis).
5. T1 (Monotonie) und die adversarialen Fälle.
6. Gate grün (`&&`-verkettet, nie in einer Pipe, `RUST_EXIT=0` **und**
   `FE_EXIT=0`).

## 8. Offene Punkte

Keine. Die Richtung folgt aus einer bestehenden Invariante („nur
Eskalation"); der Preis aus §4.2 ist benannt und bewusst getragen.

## 9. Klarstellungen

*(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)*
