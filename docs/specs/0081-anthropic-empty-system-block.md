# Spec 0081 — Anthropic: kein leerer System-Block mit Cache-Markierung

Status: umgesetzt
Zweck: „Zugangsdaten testen" mit einem Anthropic-Anbieter scheitert nicht
an einem leeren System-Block.
Bezüge: Spec 0064 (Cache-Markierungen), Spec 0050 und 0056 (Test-Knopf).
Review-Einstufung: normal.

## 1. Ausgangslage

Die Probe des Test-Knopfs schickt einen leeren System-Text und die Nachricht
„Hi". Mit nativem Tool-Calling bleibt der System-Text leer, und Anthropic
antwortet auf einen Textblock mit leerem Text und Cache-Markierung mit
HTTP 400 (`cache_control cannot be set for empty text blocks`). Die
Oberfläche meldete den Anbieter dann als nicht erreichbar, obwohl der Key
gültig war. Der reguläre Chat war nie betroffen: Sein System-Prompt beginnt
immer mit einem festen, nicht leeren Text.

## 2. Ziel und Nicht-Ziele

Ziel: Ist der fertige System-Text leer, enthält der Request **kein**
System-Feld.

Nicht-Ziele: Änderungen an der Probe selbst, an den Werkzeug-Breakpoints
oder an der Caching-Strategie für nicht leere Prompts.

## 3. Anforderungen

**A1.** Ist der System-Text nach dem optionalen Fallback-Zusatz leer oder
besteht nur aus Leerraum (auch `"  \n"`), wird das System-Feld im Request
weggelassen. Sonst bleibt es unverändert, samt Cache-Markierung.

**A2.** Die Cache-Markierung gilt „unbedingt" mit genau dieser Ausnahme für
leeren Text.

**A3.** Alle anderen Felder des Requests bleiben unverändert (Modell,
Längenlimit, Nachrichten, Werkzeuge samt Breakpoint am letzten Werkzeug).

## 4. Invarianten

- Ein nicht leerer System-Prompt trägt weiter genau eine Cache-Markierung.
- Der Request enthält nie einen Textblock mit leerem Text und
  Cache-Markierung.

## 5. Akzeptanzfälle

- T1 Natives Tool-Calling, leerer System-Text → kein System-Feld.
- T2 Fallback-Modus, leerer System-Text → System-Feld mit dem nicht leeren
  Fallback-Zusatz und Cache-Markierung (Wächter: der Fallback bleibt
  gecacht).
- Die bestehenden Fälle zur Cache-Markierung bleiben unverändert grün.

Manueller Test: Einstellungen → Anthropic-Anbieter → „Zugangsdaten testen"
meldet Erfolg.
