# Spec 0079 — Karte „Notiz ist sehr groß": Schließen, Später, 10 000 Zeichen

Status: umgesetzt
Zweck: Die Karte, die beim Verbindungsende eine große Notiz meldet, lässt
sich schließen und für die laufende App-Sitzung zurückstellen, und sie
erscheint erst bei wirklich großen Notizen.
Bezüge: Spec 0057 (Kürzungs-Vorschlag beim Verbindungsende), Spec 0058
(Hinweis im Editor, „Mache ich selbst"), Spec 0010 (Notiz-Vorschlag beim
Beenden), ADR 0050, ADR 0070.

## 1. Überblick

Ist die gespeicherte Notiz eines Servers beim Verbindungsende groß,
erscheint je Server eine Karte „Notiz für Server „…" ist sehr groß" mit
„Mache ich selbst" und „Ja, zusammenfassen" (Spec 0057). Hat am selben
Verbindungsende schon ein Notiz-Vorschlag der KI geöffnet (Spec 0010),
erscheint die Karte nicht. Diese Spec regelt, wie der Nutzer die Karte
los wird und ab welcher Größe sie erscheint.

## 2. Ziel und Nicht-Ziele

Ziel: Die Karte lässt sich schließen und zurückstellen; die Schwelle ist in
Zeichen bemessen und überall dieselbe.

Nicht-Ziele: „Später" über einen Neustart der App hinaus; Änderungen an
der Zusammenfassung selbst oder an der Kontext-Kürzung.

## 3. Anforderungen

**A1 — Schließen.** Jede Karte hat oben rechts einen Knopf ✕ mit der
Bezeichnung „Hinweis schließen", gestaltet wie der ✕-Knopf der
Fehler-Karten. Ein Klick entfernt nur diese Karte. Beim nächsten
Verbindungsende desselben Servers darf sie wieder erscheinen.

**A2 — Später.** Jede Karte hat einen Knopf „Später", gestaltet wie „Mache
ich selbst" und links davon. Ein Klick entfernt die Karte und stellt den
Server zurück: Bis zum Neustart der App erscheint für diesen Server keine
solche Karte mehr. Andere Server sind nicht betroffen.

**A3 — Dauer von „Später".** Die Zurückstellung gilt für die laufende
App-Sitzung. Sie übersteht Wechsel zwischen Bildschirmen und ein neues
Aufbauen der Oberfläche, wird aber nirgends gespeichert (weder im Browser-
noch im App-Speicher) und endet mit dem Neustart der App.

**A4 — Schwelle in Zeichen.** Eine Notiz gilt als groß ab **10 000
Zeichen** (Unicode-Zeichen, nicht Bytes und nicht UTF-16-Einheiten; ein
Emoji zählt als ein Zeichen, ein „ä" ebenso). Ab genau 10 000 Zeichen
erscheint die Karte. Dieselbe Schwelle gilt für den Hinweis im
Notiz-Editor (Spec 0058, Teil 1). Das Frontend bekommt sie von der App
und hat keine eigene zweite Zahl. Die Schwelle liegt bewusst deutlich
über der Obergrenze einer KI-Zusammenfassung, damit normal genutzte
Notizen nicht auslösen.

**A5 — Bestehendes Verhalten bleibt.** „Mache ich selbst", „Ja,
zusammenfassen", die Regel „höchstens eine Karte je Server" und die
Fehler-Karten verhalten sich unverändert. „Ja, zusammenfassen" und „Mache
ich selbst" stellen den Server **nicht** zurück.

## 4. (entfallen)

## 5. Invarianten

- Keiner der Knöpfe verändert die gespeicherte Notiz.
- Die Schwelle hat genau eine Quelle.
- Nichts aus A2/A3 wird gespeichert.

## 6. Akzeptanzfälle

- T1: Karte für Server A → „Hinweis schließen" → Karte weg; nächstes
  Verbindungsende von A → Karte wieder sichtbar.
- T2: Karte für A → „Später" → Karte weg; nächstes Verbindungsende von A →
  keine Karte; Verbindungsende von B → Karte für B.
- T3: „Später" für A übersteht ein Aus- und Wiedereinhängen der
  Oberfläche: danach für A keine Karte.
- T4: „Mache ich selbst" stellt nicht zurück: danach erscheint die Karte
  für A wieder.
- Editor-Hinweis bei Schwelle 10: 5 × „ä" → kein Hinweis; 6 × „😀" → kein
  Hinweis; 10 × „ä" → Hinweis.
- T5: 9 999 Zeichen → keine Karte; 10 000 Zeichen → Karte.
- T6: 6 000 × „ä" (12 000 Byte, 6 000 Zeichen) → keine Karte.
