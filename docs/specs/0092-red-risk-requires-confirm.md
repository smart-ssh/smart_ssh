# Spec 0092 — Rotes Risiko verlangt Bestätigung

Status: umgesetzt
Zweck: Ein rot eingestufter Vorschlag läuft nie ohne Rückfrage, solange die App-Einstellung „Bei rotem Risiko immer nachfragen" an ist (Standard), auch nicht gegen eine Allow-Regel.
Bezüge: Spec 0002 (Filter-Engine), Spec 0026 (Risiko-Einstufung und Zweitmeinung), Spec 0028 (MCP), Spec 0074 (Auswertung der Zweitmeinung), ADR 0084.

## 1. Ausgangspunkt

Die Risiko-Einstufung (Spec 0026) hat zwei Achsen, Server-Risiko und
Daten-Risiko, je mit den Stufen keine, gelb, rot. Ohne diese Spec wäre sie ein
reiner Hinweis: Ein Kommando, das rot eingestuft ist und auf das eine
Allow-Regel passt, liefe automatisch.

Die Einstufung gibt es für Aktionen mit Pseudokommando: Kommandos
(`SuggestCommand`), Datei lesen und Datei schreiben. Notiz-Änderungen und
Dokument-Erzeugung haben keine.

Die Einschätzung der Zweitmeinung (Spec 0026, Abschnitt 3) kommt erst nach der
regelbasierten Einstufung. Die Karte zeigt zuerst die regelbasierte
Einstufung; die Zweitmeinung wird danach eingeholt.

## 2. Verhalten

Mit der Einstellung „Bei rotem Risiko immer nachfragen" (Standard an) wird jeder
Vorschlag mit Rot auf **einer** der beiden Achsen bestätigungspflichtig. Das
gilt auch dann, wenn erst die KI-Zweitmeinung auf Rot hebt.

**Nicht-Ziele:** Die Einstufung, ihre Muster und Gelb ändern sich nicht. Es
gibt keine Einstellung je Server. `Deny` bleibt `Deny`, und keine bestehende
Eskalation entfällt, auch nicht bei ausgeschalteter Einstellung.

## 3. Anforderungen

**A1 — Einstellung**

- A1.1 Die Einstellung ist app-weit und boolesch.
- A1.2 Fehlt der Wert oder ist er kein boolescher Wert (z. B. die Zeichenkette
  `"false"` oder `0`), gilt „an". Das ist fail-safe und betrifft auch
  bestehende Installationen.
- A1.3 Der Wert wird beim Verbinden gelesen und gilt für die Dauer der
  Sitzung. Eine Änderung wirkt ab der nächsten Verbindung; der Hinweistext am
  Schalter sagt das.
- A1.4 Der Schalter steht im Einstellungsbereich des Risiko-Klassifizierers und
  lässt sich unabhängig davon bedienen, ob die Zweitmeinung an ist. Die Texte
  gibt es auf Deutsch und Englisch.

**A2 — Eskalation bei regelbasiertem Rot**

- A2.1 Ist die Einstellung an, die Entscheidung `AutoExec` und mindestens eine
  Achse rot, wird die Entscheidung `Confirm` mit dem Code
  `FILTER_RED_RISK_REQUIRES_CONFIRM` und einem Grund, der die rote Achse und
  deren Begründung nennt. Der übersetzte Text zu diesem Code ist so formuliert, dass
  er auch den Fall „nicht einschätzbar" (Kommando über dem Längenlimit)
  abdeckt.
- A2.2 Das gilt für den Chat und für MCP gleichermaßen.
- A2.3 Das Verdachts-Kennzeichen für eingeschleuste Anweisungen wird dabei nur
  gelesen, nicht verbraucht. Ist es gesetzt, wird der Injection-Grund gezeigt,
  wie bei Secret-Pfad und `sftp-server`.
- A2.4 Der Secret-Pfad-Grund und der `sftp-server`-Grund haben Vorrang. Greift
  einer davon, bleibt dessen Code.
- A2.5 Ist die Einstellung aus, sind Entscheidung und Code wie ohne diese Spec.

**A3 — Nachträgliches Rot durch die Zweitmeinung**

- A3.1 Ist die Einstellung an, die Entscheidung nach allen anderen
  Eskalationen `AutoExec` und hebt die Zweitmeinung das Daten-Risiko auf Rot,
  wird die Aktion **nicht** automatisch ausgeführt, sondern wie ein `Confirm`
  mit `FILTER_RED_RISK_REQUIRES_CONFIRM` behandelt: gleiche Wartezeit, gleiche
  Abbruchlogik, gleicher Hinweis am Hintergrund-Tab und gleiche
  Ledger-Einträge (bestätigt/abgelehnt, mit Code) wie bei jedem anderen
  `Confirm`.
- A3.2 Die Oberfläche erfährt die geänderte Entscheidung über ein eigenes
  Ereignis, das nach der Aktualisierung der Einstufung gesendet wird. Die
  Karte zeigt danach den Bestätigungsdialog mit dem übersetzten Grund; ein
  Klick darauf erreicht die wartende Aktion. Die wartende Bestätigung ist
  deshalb schon vor dem Ereignis registriert, sodass ein früher Klick nicht
  verloren geht.
- A3.3 Ein Stopp der automatischen Fortsetzung hat Vorrang. Eine gestoppte
  Aktion wird wie sonst übersprungen und bekommt keinen Dialog.
- A3.4 Eine Ablehnung setzt wie jede andere die „frühere Ablehnung".
- A3.5 Hebt die Zweitmeinung nur auf Gelb, bleibt sie aus, oder ist die
  Einstellung aus, ändert sich nichts.
- A3.6 Auch der Tab-Zustand (Hinweis auf eine wartende Aktion, Ablehnen beim
  Schließen des Tabs) reagiert auf das Ereignis, nicht nur die Karte.

**A4 — Dokumentation**

- A4.1 ADR 0084 hält Entscheidung, Standard, das Verhalten bei der Zweitmeinung
  und die Restfälle (Abschnitt 6) fest.
- A4.2 Die README beschreibt die Einstellung, ihren Standard und dass sie auch
  eine Allow-Regel übersteuert.
- A4.3 Die Doc-Kommentare des Risiko-Moduls sagen nicht mehr „beeinflusst nie
  die Entscheidung", sondern verweisen auf diese Spec und ADR 0084.

## 4. Verhaltensdetails

- **Stelle in der Eskalationskette:** nach dem `sftp-server`-Grund, vor dem Grund
  für eingelesenen Serverinhalt. So bleibt A2.4 erfüllt, und das Kennzeichen
  wird nur gelesen (A2.3).
- **Anzeige des Grunds:** Der Dialog zeigt wie bei allen bekannten Codes den
  festen übersetzten Text zum Code. Die rote Achse samt Musterbegründung steht
  im Grund für Ledger und KI-Kontext; im Dialog sieht der Nutzer sie am Badge.
- **Grund im Fall A3:** Er nennt nur die rote Achse, nicht die Begründung der
  KI-Zweitmeinung. Der Grund wird unredigiert im Ledger gespeichert, und der
  Modelltext kann Teile des Kommandos zitieren, etwa ein Passwort. Der Text
  der Zweitmeinung (`risk-assessment-updated`) wird nur angezeigt, nicht
  gespeichert und nicht als HTML gerendert.
- **Hard-Blacklist:** Sie liefert `Confirm` mit ihrem eigenen Code, nicht
  `Deny`. Die Eskalation greift nur auf `AutoExec` und lässt diesen Code
  unberührt.
- **Kommando über dem Längenlimit des Filters:** Es gilt bei eingeschalteter
  Einstellung als rot, weil seine Einstufung nicht belastbar ist (nur
  Eskalation).

## 5. Sicherheitszusagen

- **Nur Eskalation:** Die Eskalation und A3 machen nur aus `AutoExec` ein
  `Confirm`. Kein Pfad erzeugt `AutoExec` oder hebt `Deny` auf.
- **Verdachts-Kennzeichen:** Es wird nicht zusätzlich verbraucht (A2.3).
- **Bestätigung geht nicht verloren:** Die Registrierung liegt vor dem Ereignis
  (A3.2) und wird auf jedem Ausgang abgeräumt (Spec 0088).
- **Fail-safe:** Eine unlesbare oder fehlende Einstellung bedeutet „an" (A1.2).
- **Keine neue Datensenke:** Der Grundtext enthält die Musterbegründung des
  Klassifizierers, keine Kommandoausgabe.

## 6. Grenzen

- Nach dem Ausschalten gilt Rot wieder nur als Hinweis. Eine laufende Sitzung
  behält ihren Wert bis zur nächsten Verbindung.
- Der Weg „Bearbeiten und ausführen" im Dialog läuft nicht durch die
  Risiko-Kette.
- Die Muster des Klassifizierers sind nicht vollständig (Spec 0026,
  Abschnitt 2): Ein inhaltlich rotes Kommando ohne passendes Muster löst die
  Rückfrage nicht aus.
- Harmlose, aber rot eingestufte Leser wie `cat ~/.ssh/id_rsa.pub` führen zu
  Rückfragen.
- Eine Allow-Regel auf ein rot eingestuftes Kommando wirkt bei eingeschalteter
  Einstellung nie automatisch.

## 7. Prüffälle

Alle Backend-Fälle laufen mit einer Allow-Regel, die das Kommando sonst
automatisch ausführen ließe. Als „rotes Kommando" dienen Beispiele, die der
regelbasierte Klassifizierer tatsächlich rot einstuft und die weder an der
Hard-Blacklist noch an Secret-Pfad oder `sftp-server` hängen.

| # | Fall | Erwartet |
|---|---|---|
| T1 | Server-Rot, Einstellung an | `Confirm` mit `FILTER_RED_RISK_REQUIRES_CONFIRM`, keine Ausführung vor dem Klick |
| T2 | Daten-Rot (Server nicht rot), an | wie T1 |
| T3 | T1 und T2 mit Einstellung aus | `AutoExec` |
| T4 | nur Gelb, an | `AutoExec` |
| T5 | Rot mit Deny-Regel | `Deny` bleibt |
| T5b | Rot und Hard-Blacklist | Code bleibt der der Hard-Blacklist |
| T6 | Rot und gesetztes Verdachts-Kennzeichen | Injection-Code; Kennzeichen danach noch gesetzt; die nächste harmlose Aktion mit Allow-Regel wird per Injection eskaliert |
| T7 | Secret-Pfad-Lesen, das zugleich rot ist | Secret-Code |
| T8 | MCP, rot | Code `FILTER_RED_RISK_REQUIRES_CONFIRM` |
| T8b | `sudo <rotes Kommando>`, Allow-Regel auf die Form ohne `sudo`, kein gespeichertes Passwort | Code `FILTER_RED_RISK_REQUIRES_CONFIRM` |
| T9 | Datei lesen auf einen rot eingestuften Pfad, der kein Secret-Pfad ist | Code `FILTER_RED_RISK_REQUIRES_CONFIRM` |
| T10 | Zweitmeinung (Mock) hebt Gelb auf Rot, an | keine Ausführung; Ereignis nach der Aktualisierung der Einstufung; Bestätigen führt genau einmal aus; Ledger „bestätigt" mit Code |
| T11 | wie T10, Ablehnen | keine Ausführung, „frühere Ablehnung" gesetzt, Ledger „abgelehnt" |
| T12 | wie T10, Einstellung aus | Ausführung wie sonst, kein neues Ereignis |
| T13 | Zweitmeinung hebt nur auf Gelb, an | Ausführung, kein neues Ereignis |
| T14 | wie T10, Stopp während der Zweitmeinung | übersprungen, kein Dialog |
| T15 | wie T10, Zeitüberschreitung beim Klick | wie jede Bestätigung: nichts ausgeführt |
| T16 | Einstellung: Wert fehlt / `false` / `"false"` | an / aus / an |
| T17 | mehrzeiliges Skript mit einer roten Zeile, Allow-Regel | Stuft der Klassifizierer es rot ein, `Confirm`; sonst ist das ein Befund zur Klassifizierer-Grenze |
| U1 | Oberfläche: Karte erhält das Ereignis | zeigt Dialog mit übersetztem Text; Klick sendet die Antwort |
| U2 | Oberfläche: Hintergrund-Tab erhält das Ereignis | Hinweis auf wartende Aktion; Tab schließen lehnt genau diese Aktion ab |
| U3 | Oberfläche: Schalter — Wert fehlt / `false` / kein boolescher Wert | an / aus / an, wie im Backend (A1.2) |
| U4 | Sprachdateien de/en enthalten den neuen Code | Parität |
