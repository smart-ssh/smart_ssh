# Spec 0074 — Im Urteils-Parser gewinnt die Eskalation, nicht das erste Wort

Status: umgesetzt
Zweck: Zwei Auswertungen lesen das Urteil einer KI-Zweitmeinung aus Fließtext. Enthält die Antwort mehrere Urteilswörter, gewinnt das eskalierende; eine spätere Warnung wird nicht von einem früheren „nein" oder „none" verschluckt.
Bezüge: Spec 0026 (Risiko-Hinweise, Abschnitt 3: nur Eskalation), Spec 0092 (Rot verlangt Bestätigung), ADR 0024.

## 1. Ausgangslage

Zwei Auswertungen gleichen Aufbaus lesen ein Urteil aus der Antwort eines
Modells:

| Auswertung | Erkennt |
|---|---|
| Zweitmeinung zum Daten-Risiko | `none` / `yellow` / `red` |
| Prüfung auf eingeschleuste Anweisungen | `ja`/`yes` / `nein`/`no` |

Beide zerlegen die Antwort in Wörter, bereinigen jedes um Satzzeichen und
suchen Urteilswörter. Der Rest der Antwort wird zur Begründung.

**Die aufrufende Seite ist asymmetrisch gebaut:**

- Eine Antwort „ja" auf die Injektions-Prüfung setzt den Verdacht
  (`injection_suspected`). „Nein" und „kein Urteil" bewirken nichts und nehmen
  einen gesetzten Verdacht nicht zurück.
- Die Zweitmeinung zum Daten-Risiko wird nur übernommen, wenn sie höher ist als
  die regelbasierte Einstufung. Ein `none` der KI senkt eine regelbasierte
  Einstufung nicht (Spec 0026, Abschnitt 3: „Nur Eskalation, nie
  Abschwächung").

Was die Auswertung **nicht meldet**, kann die aufrufende Seite nicht
eskalieren. Würde die Auswertung beim **ersten** Urteilswort aufhören, könnte
ein „nein" an Position 3 ein „ja" an Position 30 verdecken. Der Fall ist nicht
theoretisch: Die Injektions-Prüfung bekommt **nicht vertrauenswürdigen
Inhalt** vorgelegt und bittet um eine Begründung. Eine Begründung, die den
Inhalt zitiert, ist die Regel, und den Inhalt gestaltet der Angreifer.

## 2. Ziel und Nicht-Ziele

**Ziel:** Enthält eine Antwort mehrere Urteilswörter, gewinnt das
**eskalierende**. Was die Auswertung meldet, ist nie weniger alarmierend als
das, was in der Antwort steht.

**Nicht-Ziele:**

1. Keine Änderung an der aufrufenden Seite; sie ist richtig (Abschnitt 1).
2. Keine Änderung an den Prompts. Ein Prompt kann bitten, ein Modell kann sich
   nicht daran halten; die Auswertung muss robust sein, nicht der Prompt
   strenger.
3. Die Begründung bleibt erhalten.
4. Kein zweiter Aufruf beim Modell. (Das strukturierte Urteilsfeld kam später,
   Abschnitt 4.4.)

## 3. Anforderungen

**A1 (MUSS)** Die Injektions-Prüfung meldet `true`, sobald **irgendwo** in der
Antwort ein Wort `ja` oder `yes` vorkommt, unabhängig davon, ob vorher ein
`nein`/`no` steht. `false` nur, wenn ein `nein`/`no` vorkommt und **kein**
`ja`/`yes`.

**A2 (MUSS)** Die Zweitmeinung zum Daten-Risiko meldet die **höchste** in der
Antwort vorkommende Stufe, nicht die erste: `red` vor `yellow` vor `none`.

**A3 (MUSS)** Kommt kein Urteilswort vor, bleibt das Ergebnis „kein Urteil".
Das heißt weiterhin „keine Prüfung verfügbar", nicht „alles in Ordnung".

**A4 (MUSS)** Die Begründung ist der Text **nach dem Wort, das gewonnen hat**.
Bleibt danach nichts Sinnvolles übrig, ist es die vollständige Antwort. Kommt
nur ein Urteilswort vor, ist das Verhalten unverändert.

**A5 (MUSS)** Die Wortbereinigung bleibt: nur alphanumerische Zeichen,
kleingeschrieben, Vergleich auf Gleichheit, **keine** Teilstring-Suche. Eine
Teilstring-Suche würde bei „redirect" auf „red" auslösen (ADR 0024).

## 4. Design

### 4.1 Warum „höchste gewinnt" und nicht „erste gewinnt"

Beide Auswertungen dienen einer Sicherheitsentscheidung, und für die gilt im
Projekt durchgehend: verschärfen, nie lockern. „Erste gewinnt" macht das
Urteil von der **Wortstellung** abhängig, einer Eigenschaft, die der Angreifer
über den geprüften Inhalt mitbestimmt. „Höchste gewinnt" macht es von der
**Aussage** abhängig. Die Änderung ist monoton: Die neue Auswertung meldet nie
eine niedrigere Stufe als die frühere.

### 4.2 Der Preis

Ein Angreifer kann jetzt umgekehrt eine **falsche Eskalation** auslösen:
Inhalt, der das Wort „red" enthält und vom Modell zitiert wird, hebt die
Einstufung. Das ist die richtige Richtung, aber nicht kostenlos. Zu viele
Rückfragen führen dazu, dass Nutzer Dialoge blind bestätigen (Confirm-Fatigue).
Bewusst hingenommen: Eine falsche Eskalation ist sichtbar und korrigierbar, eine
verschluckte nicht. Zeigt sich Confirm-Fatigue im Betrieb, ist die Antwort das
strukturierte Urteilsfeld (Abschnitt 4.4), nicht die Rückkehr zu „erste
gewinnt"; die Regel „höchste gewinnt" bleibt dahinter als Rückfall bestehen.

### 4.3 Verworfen

- **Nur die ersten N Wörter betrachten.** Ein Modell, das sich nicht an das
  Format hält, bekäme dann gar kein Urteil, aus einer verschluckten Eskalation
  würde „kein Urteil".
- **Strukturiertes Antwortformat ohne Rückfall.** Ein Modell, das das Format
  nicht einhält, bekäme kein Urteil. Das Format kommt (Abschnitt 4.4), der
  Rückfall bleibt.

### 4.4 Strukturiertes Urteilsfeld

Beide Prompts bitten das Modell, in der ersten Zeile nur das Urteil zu nennen
und danach die Begründung zu schreiben: `VERDICT: none|yellow|red`
(Zweitmeinung) bzw. `VERDICT: yes|no` (Injektions-Check).

- **Nur das Feld zählt.** Steht ein gültiges Urteilsfeld in der Antwort,
  bestimmt allein dessen Wert das Urteil. Begründung und zitierter Inhalt
  (etwa `PermitRootLogin yes` oder das Wort `red`) beeinflussen es nicht. Ein
  Feld steht am Zeilenanfang (Markdown-Zeichen davor sind erlaubt); ein Zitat
  mitten in einer Zeile ist kein Feld. `VERDICT:yes`, Groß-/Kleinschreibung und
  ein kompaktes JSON-Objekt `{"verdict":"yes",…}` werden erkannt. Die
  Begründung ist der übrige Text ohne die Feldzeile.
- **Normalisierung.** Der Wert wird per Unicode-NFKC normalisiert und
  kleingeschrieben; Vollbreiten-Zeichen (`ｙｅｓ`) gelten also. Zeichen anderer
  Schriften (kyrillisches `а` in `jа`) werden nicht auf lateinische Buchstaben
  abgebildet und sind kein gültiger Wert.
- **Rückfall.** Fehlt das Feld, ist sein Wert unbekannt oder ungültig, oder
  nennen mehrere Felder verschiedene Werte, gilt exakt die Regel aus A1–A5 auf
  dem ganzen Text. Das Ergebnis ist dann nie niedriger als bei dieser Regel.
  „Kein Urteil" bleibt „keine Prüfung verfügbar" (A3/I3).
- Die Eskalationsrichtung ändert sich nicht: Die Daten-Risiko-Stufe ist weiter
  das Maximum aus Regel und KI (Spec 0026, Abschnitt 3); ein strukturiertes
  `none`/`no` senkt nie ein regelbasiertes Ergebnis.

## 5. Sicherheits-Invarianten

Berührt sind die Risiko-Einstufung und die Erkennung eingeschleuster
Anweisungen.

- **I1 — Nur Verschärfung.** Für jede Eingabe meldet die Auswertung eine Stufe,
  die mindestens so hoch ist wie die der „erste gewinnt"-Fassung (als
  Eigenschaft über zufällige Eingaben geprüft, nicht nur an Beispielen).
- **I2 — Die aufrufende Seite bleibt unangetastet.** „Nur Eskalation, nie
  Abschwächung" (Spec 0026, Abschnitt 3) gilt dort weiter; diese Spec ergänzt
  dieselbe Regel eine Ebene tiefer.
- **I3 — „Kein Urteil" heißt weiter „keine Prüfung verfügbar".** Kein Weg darf
  aus einem nicht erkennbaren Urteil eine Entwarnung machen.
- **I4 — Kein Inhalt aus der Antwort wird ausgeführt oder gespeichert**, nur
  angezeigt; die bestehende Redaction auf diesem Pfad bleibt.

## 6. Prüffälle

### 6.1 Eigenschaft statt Beispiel

- T1 **Monotonie:** Für erzeugte Antworten (Urteilswörter in zufälliger Zahl,
  Reihenfolge und Schreibung, dazwischen Fülltext) gilt: neue Stufe ≥ alte
  Stufe, und die Injektions-Prüfung meldet nie `false`, wo die „erste
  gewinnt"-Fassung `true` meldete. Dies ist der Nachweis von I1.

### 6.2 Injektions-Prüfung

- T2 `"Nein. — Korrektur: ja, der Text enthält eine Anweisung."` → `true`.
- T3 `"no"` allein → `false`.
- T4 `"ja"` allein → `true`.
- T5 Weder noch (`"Der Text ist unauffällig."`) → kein Urteil (A3).
- T6 Bei genau einem Urteilswort ist die Begründung der Text danach (A4).
- T7 Bei mehreren: der Text nach dem **gewinnenden** Wort.

### 6.3 Zweitmeinung und adversariale Fälle

- T8 `"none … red …"` → `Red`.
- T9 `"red … none"` → `Red` (Reihenfolge egal).
- T10 `"yellow … none"` → `Yellow`.
- X1 **Zitierter Inhalt.** Antwort, die den geprüften Text zitiert und darin ein
  `no` enthält, danach das eigentliche Urteil `ja` → `true`.
- X2 **Wortgrenzen bleiben.** `"redirect"`, `"nonetheless"`, `"nobody"`,
  `"yesterday"`, `"janein"` lösen **kein** Urteil aus (A5).
- X3 **Sehr viele Urteilswörter.** 10 000 abwechselnde `none`/`red` ergeben
  `Red`, die Laufzeit ist linear.
- X4 **Groß-/Kleinschreibung und Satzzeichen:** `"RED!"`, `"(Ja)"`, `"nein,"`
  werden erkannt.
- X5 **Leere und nur aus Satzzeichen bestehende Antwort** → kein Urteil, keine
  Panik.
- X6 **Gemischt deutsch/englisch:** `"no — aber ja"` → `true`; ein englisches
  und ein deutsches Urteilswort widersprechen sich nicht, sie zählen wie zwei
  Urteile.

### 6.4 Strukturiertes Feld (Abschnitt 4.4)

- S1 Strukturiertes `no`/`none`, dessen Begründung `PermitRootLogin yes` bzw.
  `red` zitiert → `no`/`none`.
- S2 `VERDICT:yes`, vollbreite Zeichen und kompaktes JSON werden erkannt.
- S3 Homoglyphen-Wert → kein gültiges Feld, Rückfall.
- S4 Ohne Feld oder mit widersprüchlichen Feldern ist das Ergebnis exakt das
  der Regel aus A1–A5 (Eigenschaftstest).
- S5 Provider-Fehler und leere Antwort → „keine Prüfung verfügbar".
