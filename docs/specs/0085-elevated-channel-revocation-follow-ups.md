# Spec 0085 — Erhöhter Kanal: Widerruf in laufenden Befehlen, Kanal-Ende, Schutz des normalen Kanals

Status: umgesetzt
Zweck: Ein Widerruf des erhöhten Dateibrowser-Modus beendet auch Befehle, die schon laufen. Der erhöhte Kanal ist danach wirklich geschlossen, und außerhalb der Anwendungslogik lässt sich kein anderer Kanal in den normalen SFTP-Kanal einer Sitzung schieben.
Bezüge: Spec 0067 (erhöhter Modus), Spec 0084 §9 (Widerruf wirkt beim Zugriff), Spec 0020 (Dateikanal), Spec 0086 (Transport-Paarung, Transfer-Meldung), Spec 0088 (Kanal-Zugang ohne Panic), ADR 0078, ADR 0080.
Review-Priorität: erhöht (Schreibzugriffe über den erhöhten Kanal).

## 1. Verhalten im Überblick

Der erhöhte Modus (Spec 0067) kann auf vier Wegen enden: Der Nutzer schaltet
ihn aus, die Sitzung wird getrennt, er wird für einen anderen Nutzer neu
aktiviert, oder der Kanal fällt weg. In jedem Fall gilt:

- Kein Browser-Befehl führt danach noch eine SFTP-Operation mit den alten
  Rechten aus, auch wenn er vor dem Widerruf begonnen hat.
- Der Kanal ist verworfen; auf dem Server laufen weder `sudo` noch
  `sftp-server` weiter, die Verbindung selbst bleibt bestehen.
- Der normale SFTP-Kanal der Sitzung gehört der Sitzung; kein Code außerhalb
  der Anwendungslogik kann ihn setzen, ersetzen oder entnehmen. KI und MCP
  können dadurch nicht über einen untergeschobenen erhöhten Kanal laufen.

## 2. Nicht-Ziele

- Kein Zähler „x von y erledigt“ bei einem Abbruch.
- Kein neuer Nutzertext: Der Abbruch meldet den bestehenden Fehler.
- Kein Rückgängigmachen schon geänderter Einträge. Das wäre unvollständig und
  selbst ein erhöhter Schreibvorgang nach dem Widerruf.
- Der Bestätigungs-Timeout der KI-Aktionen bleibt unverändert.

## 3. Anforderungen

**A1 — Widerruf wirkt auch innerhalb eines laufenden Befehls.**

1. Nach einem Widerruf (Ausschalten, Entfernen der Sitzung nach Spec 0084
   A2.1, Neu-Aktivieren für einen anderen Nutzer) führt **kein**
   Browser-Befehl mehr eine SFTP-Operation über den widerrufenen Kanal aus.
   Das gilt auch für Befehle, die vor dem Widerruf begonnen haben. Eine schon
   laufende einzelne SFTP-Operation darf zu Ende laufen.
2. Ein so abgebrochener Befehl endet mit dem Fehler
   `ELEVATED_CHANNEL_INACTIVE` („Der erhöhte Modus ist nicht mehr aktiv …“),
   nicht mit Erfolg und nicht mit einem SFTP-Fehler. Schon geänderte Einträge
   bleiben geändert.
3. Das Ausschalten wartet höchstens auf die gerade laufende einzelne
   SFTP-Operation, nicht auf den Rest des Befehls. „Vorgang“ in Spec 0084 §9
   heißt seitdem: eine einzelne SFTP-Operation.
4. Ein abgebrochener erhöhter Lösch- oder chmod-Befehl, bei dem mindestens
   eine Operation lief, schreibt die Protokollzeile wie bei jedem Fehlschlag:
   `ok = false`, **mit** dem Zielnutzer, unter dem die schon ausgeführten
   Operationen liefen. Scheitert schon der Zugriff auf den Kanal, gibt es wie
   bisher keine Zeile. Eine Zeile `ok = false` ohne ausgeführte Operation ist
   zulässig; sie behauptet keinen Erfolg.
5. Über den normalen Kanal ändert sich nichts.
6. Kein Rückfall: Nach dem Abbruch läuft keine Operation dieses Befehls über
   den normalen Kanal weiter.

**A2 — Kanal-Ende ist gesichert.**

1. Wird eine über den erhöhten Weg geöffnete SFTP-Sitzung verworfen, schließt
   der Server den zugehörigen SSH-Kanal innerhalb einer festen Frist, während
   die Verbindung weiter steht (danach ist ein weiterer Befehl auf ihr
   möglich).
2. Nach dem Ausschalten hält kein Zustand den Kanal mehr. Er ist verworfen,
   nicht nur als widerrufen markiert, auch wenn ein wartender Befehl noch eine
   Referenz auf den Platz des Kanals hält.

**A3 — Der normale Kanal ist außerhalb der Anwendungslogik nicht
ersetzbar.**

1. Code außerhalb der Anwendungslogik kann den normalen SFTP-Kanal einer
   Sitzung weder setzen noch ersetzen noch herausnehmen. Er erhält nur
   Zugriff, um ihn zu **benutzen**, unter derselben Sperre wie sonst. Keine
   herausgegebene Referenz erlaubt ein Ersetzen, auch nicht durch Tauschen
   oder Entnehmen des Werts.
2. Befüllt wird der Kanal nur in der Anwendungslogik, aus dem Transport der
   Sitzung selbst. Eine neu gebaute Sitzung hat immer einen leeren normalen
   Kanal.
3. Tests, die einen Test-Kanal einsetzen, tun das über eine Testhilfe, die in
   Produktivbauten nicht enthalten ist. Produktivbauten aktivieren sie nie.
4. Ein automatischer Test im Gate scheitert, wenn A3.1 verletzt wird: Die
   verbotenen Fälle sind aus Sicht eines anderen Crates als
   Übersetzungsfehler belegt, jeder mit einem kompilierenden Zwilling, der
   sich nur in der verbotenen Zeile unterscheidet.

**A4 — Verweise in Kommentaren.** Entfallen: eine einmalige Aufräumaufgabe
(Verweise der Anwendungslogik auf Elemente der App-Hülle), erledigt. Die
Kennung bleibt vergeben.

**A5 — Test hängt nicht.** Entfallen: Eine Test-Eigenschaft der KI-Zweig-Prüfung
(zeitlich begrenzt), keine Produktfunktion. Die Kennung bleibt vergeben.

**A6 — Gate.** Entfallen; gilt für jede Änderung (Repo-`CLAUDE.md`).

## 4. Entscheidungen

- A1 ist eine Verhaltensanforderung: Die Prüfung deckt **jede einzelne**
  Operation über den erhöhten Kanal ab, auch in künftigen Befehlen, nicht nur
  einzelne Schleifen.
- Der Abbruch kommt beim Aufrufer wörtlich als
  `ELEVATED_CHANNEL_INACTIVE` an, auch wenn ein Befehl den SFTP-Fehler
  selbst abfängt (etwa die Existenzprüfung oder die Größenabfrage vor dem
  Lesen). Die Übersetzung sitzt zentral am Zugang zum erhöhten Kanal, nicht
  je Befehl.
- A3 verläuft an der Art der herausgegebenen Referenz: Eine Referenz auf das
  Trait-Objekt lässt sich nicht ersetzen, eine auf den umhüllenden Container
  schon.
- Verworfen: Abbruch mit Zähler; Aufräumen oder Rückgängigmachen beim
  Abbruch. Begründungen und die Sperre je Operation: ADR 0078; Aufbau der
  Sitzung: ADR 0080.

## 5. Sicherheitszusagen

- **Erhöhter Kanal nur für Browser-Befehle** (Spec 0067 A, Spec 0084 A1):
  durch A3 strenger. Eine Abweichung, die KI oder MCP einen Weg zum
  erhöhten Kanal öffnet, ist ein Blocker.
- **Widerruf wirkt beim Zugriff** (Spec 0084 §9): durch A1 vollständig.
- **Keine stillen Rückfälle:** A1.6.
- **Protokoll:** Keine Zeile behauptet einen Erfolg, der nicht stattfand.
  Keine unterschlägt eine erhöhte Änderung, die stattfand (A1.4).

## 6. Abnahmefälle

Jeder mit ⚑ markierte Fall muss gegen die genannte kaputte Variante
scheitern.

Widerruf (A1):

- **T1 ⚑** Rekursives Löschen über den erhöhten Kanal, der nach der k-ten
  Operation wartet; währenddessen Ausschalten. Nach dem Widerruf läuft keine
  weitere Operation, das Ergebnis ist **gleich** `ELEVATED_CHANNEL_INACTIVE`
  (nicht nur „enthält“; gilt auch für T2–T4, T6, T6b, T6c). Kaputt:
  Widerruf nur bei Befehlsbeginn geprüft; oder Abbruch kommt als SFTP-Fehler.
- **T2 ⚑** Wie T1 für rekursives chmod.
- **T3 ⚑** Wie T1, aber Entfernen der Sitzung; das Entfernen kehrt zurück,
  ohne auf den Befehl zu warten.
- **T4 ⚑** Wie T1, aber Neu-Aktivieren für einen anderen Nutzer. Keine
  weitere Operation läuft über den alten **oder** den neuen Kanal.
- **T5 ⚑** Das Ausschalten kehrt zurück, sobald die laufende Operation fertig
  ist, nicht erst nach allen restlichen Elementen.
- **T6 ⚑** Wie T1 für die Lösch-Vorschau: kein weiterer Lesezugriff, Fehler
  statt Zählergebnis.
- **T6b ⚑** „Dateiinhalt kopieren“ und „Lokal öffnen“: Widerruf zwischen
  Größenabfrage und Lesen. Die Datei wird nicht gelesen, das Ergebnis ist
  `ELEVATED_CHANNEL_INACTIVE`.
- **T6c ⚑** Existenzprüfung mit Widerruf davor: Ergebnis
  `ELEVATED_CHANNEL_INACTIVE`, nicht „existiert nicht“.
- **T7 ⚑** Protokoll: T1 und T2 hinterlassen je genau eine Zeile `ok = false`
  mit dem ursprünglichen Zielnutzer; bei Widerruf vor dem Zugriff auf den
  Kanal keine Zeile.
- **T8** Kein Rückfall: Ein Mock des normalen Kanals zählt in T1 0
  Operationen.
- **T9** Regression: rekursives Löschen und chmod über den normalen und den
  nicht widerrufenen erhöhten Kanal verhalten sich wie bisher und enden mit
  Erfolg.

Kanal-Ende (A2):

- **T14 ⚑** Gegen einen Test-Server im Prozess: eine über den erhöhten Weg
  geöffnete Sitzung benutzen und verwerfen; der Server sieht das Schließen
  des Kanals innerhalb der Frist, die Verbindung steht weiter. Kaputt:
  Sitzung gehalten statt verworfen; dann darf das Schließen nicht
  beobachtet werden.
- **T15 ⚑** Mock-Kanal, der sein Verwerfen meldet; der Test hält während des
  Ausschaltens selbst eine Kopie des Platzes, wie ein wartender Befehl. Nach
  dem Ausschalten ist der Kanal verworfen, obwohl die Kopie lebt.

Normaler Kanal (A3):

- **T11 ⚑** Aus Sicht eines anderen Crates übersetzt nicht: Zuweisung an den
  Kanal, `replace`/`swap`/`take` über eine erhaltene Referenz. Kaputt:
  Feld wieder öffentlich oder Zugriff liefert eine Referenz auf den
  Container.
- **T11b** Bestehende Tests, die einen Kanal einsetzen, laufen über die
  Testhilfe unter gleichem Namen grün; Produktivbauten enthalten die Testhilfe
  nicht.
