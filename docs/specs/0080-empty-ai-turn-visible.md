# Spec 0080 — Leere KI-Runde: nie mehr still verschwinden

Status: umgesetzt
Zweck: Liefert das Modell in einer Runde keinen Text, sieht der Nutzer einen
Hinweis. Eine leere, wegen des Längenlimits abgeschnittene Runde versucht
die App einmal mit mehr Budget.
Bezüge: Spec 0065 (Längenlimit, Abschneide-Retry), Spec 0062 (Abbruchgrund),
Spec 0006.
Review-Einstufung: erhöht (berührt den Retry-Pfad, der abgeschnittene
Kommandovorschläge zurückhält; adversariale Fälle in Abschnitt 5).

## 1. Ausgangslage

Bei einem OpenAI-kompatiblen Endpunkt mit einem Modell mit Denkphase
blieben mehrere Nutzer-Nachrichten ohne Antwort: Die Runden endeten mit
`finish_reason: length` oder `stop`, ohne dass ein Text entstand und ohne
dass der Nutzer etwas sah. Die Denk-Tokens (`reasoning_content`/`reasoning`)
werden nicht als Text weitergegeben. Eine leere Runde wurde weder
gespeichert noch angezeigt.

## 2. Ziel und Nicht-Ziele

Ziel: A1–A4.

Nicht-Ziele: Denkinhalte anzeigen oder speichern; den Hinweis persistieren
(er gilt nur für die laufende Ansicht); Änderungen am Anthropic-Provider;
Verhalten bei Verbindungsabbruch, fehlendem `finish_reason` oder
`content_filter` (bleibt wie zuvor).

## 3. Anforderungen

**A1 — Retry bei leerer, abgeschnittener Runde.** Endet eine Runde des
OpenAI-kompatiblen Anbieters mit `finish_reason: "length"`, ist bis dahin
**kein** Text angefallen (in beiden Modi) und gibt es weder Kommandovorschläge
noch eine erkannte Aktion, gilt die bestehende Retry-Logik: einmal, Budget
verdoppelt und am Modell-Maximum gedeckelt; ein zweiter Fehlschlag endet
mit „Antwort abgeschnitten" (ohne „Weiter"). Nur in diesem Fall; alle
anderen Endungen verhalten sich wie zuvor.

**A2 — Leere Runde melden.** Endet eine Runde regulär, ohne dass in dieser
Runde Text angefallen oder eine Aktion vorgeschlagen wurde, meldet das
Backend das der Oberfläche. Nichts davon landet in Ledger oder Verlauf. Auf
einen Fehler folgt keine solche Meldung.

Die Meldung gilt nur für eine Runde, die eine Nutzer-Nachricht beantwortet:
Runde 1 eines Turns (neue Nachricht oder „Weiter") sowie jede spätere
Runde, in die eingereihte Nutzer-Nachrichten eingespeist wurden. Eine
automatische Folgerunde nach einer ausgeführten oder blockierten Aktion,
die ohne Text endet, bleibt still.

**A3 — Leere Runde zeigen.**

- Bei einer leeren Runde zeigt der Chat einen Hinweis: „Das Modell hat
  keine Antwort geliefert. Bei Modellen mit Denkphase hilft ein höheres
  Ausgabe-Limit in den Provider-Einstellungen." Er hat **keinen**
  „Weiter"-Knopf (der Fortsetzungstext passt nicht) und keine Leiste für
  Export oder Notiz. Der Nutzer schreibt einfach weiter.
- Kommt eine „abgeschnitten"-Meldung ohne vorheriges Assistenten-Element,
  erscheint ein leeres Assistenten-Element mit Kürzungs-Hinweis und
  „Weiter", statt dass die Meldung verworfen wird. Export und Notiz
  erscheinen bei leerem Text nicht. „Weiter" gilt für eine leere,
  abgeschnittene Runde anderer Anbieter; beim OpenAI-kompatiblen Anbieter
  folgt nach A1 Text oder die Fehlermeldung.
- Text der nächsten Runde landet nie in einem solchen Hinweis, sondern in
  einem neuen Assistenten-Element.

**A4 — Messbar machen.** Am Ende jeder Runde des OpenAI-kompatiblen
Anbieters stehen im INFO-Log die Länge des Textes (auch bei 0) und die Länge
der Denk-Deltas. Denk-Deltas werden gezählt, aber nie als Text
weitergegeben und nie dem Aktions-Parser des Fallback-Modus zugeführt.

## 4. Invarianten

- Denkinhalte erreichen weder Oberfläche, Ledger, Verlauf, Aktions-Parser
  noch Log; geloggt wird nur ihre Länge.
- Der Retry aus A1 wiederholt nie eine Runde, die schon Text geliefert hat.
- Höchstens ein Retry je Anfrage.
- Abgeschnittene Kommandovorschläge werden weiterhin nie weitergegeben.

## 5. Akzeptanzfälle

Anbieter:

- T1 Nur `length`, kein Inhalt → Retry statt „abgeschnitten".
- T2 `length` mit Text → „abgeschnitten", kein Retry (Wächter).
- T3 Zweimal leer mit `length` → zwei Requests, der zweite mit
  verdoppeltem Budget, am Ende „Antwort abgeschnitten".
- T4 Wie T1 im Fallback-Modus.
- T5–T9 (Wächter gegen zu breite Umsetzung): leer mit `content_filter` →
  kein Retry; Verbindungsende ohne `finish_reason` → kein Retry; nur
  Leerzeichen mit `length` → kein Retry (es gab Text); `length` mit halbem
  Kommandovorschlag → bestehender Schutz, nichts wird weitergegeben;
  Fallback-Modus mit gültig aussehendem Aktionsblock in einem Denk-Delta
  und leerem Inhalt → keine Aktion, kein Text.
- T10 Nur Denk-Deltas, dann `stop` → kein Text, Denk-Länge > 0 und
  Text-Länge 0 im Rundenabschluss.
- T17 Ein unbekanntes Modell an einem Nicht-OpenAI-Endpunkt ohne Override
  schickt ein Limit von 8192 (Maximum 16 384, siehe Abschnitt 8).

Orchestrierung:

- T11 Runde nur mit Ende → eine Leer-Meldung, keine Ledger-Zeile.
- T12 Text und Ende → keine Leer-Meldung. T13 Aktion und Ende ohne Text →
  keine. T14 Fehler → keine.
- Aktion in Runde 1 ausgeführt oder blockiert, Folgerunde ohne Text →
  keine Leer-Meldung; eingereihte Nachricht in einer Runde > 1, die ohne
  Text endet → Leer-Meldung.

Oberfläche:

- T15 Leer-Meldung → Hinweis ohne „Weiter" und ohne Export-/Notiz-Leiste;
  ein danach eintreffendes Text-Delta erscheint in einem neuen Element.
- T16 „Abgeschnitten" direkt nach der Nutzer-Nachricht → Kürzungs-Hinweis
  mit „Weiter".

Manueller Test: Modell mit Denkphase, kleines Override-Limit (z. B. 300),
längere Frage → Antwort nach dem Retry oder sichtbarer Hinweis, nie
nichts.

## 8. Klarstellungen

- **Budget für unbekannte Modelle:** An Endpunkten außer der offiziellen
  OpenAI-API gilt für unbekannte Modelle ein Maximum von **16 384** und ein
  Standard von **8192**; der Retry aus A1 verdoppelt also bis 16 384. Ein
  HTTP 400 droht vor allem bei selbstgehosteten Servern mit kleinem
  Kontext; dort hilft das Override-Feld (Spec 0065, Abschnitt 4).
- Die Texte der Hinweise sind fest deutsch, wie der bestehende
  Kürzungs-Hinweis.
