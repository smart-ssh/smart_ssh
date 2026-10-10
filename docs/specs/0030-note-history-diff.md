# Spec 0030 — Diff-Anzeige in der Notiz-Historie

Status: umgesetzt
Zweck: Die Notiz-Historie zeigt je Revision, was sich gegenüber der
vorherigen geändert hat.
Bezüge: Spec 0003 (Notiz-Historie), Spec 0008 (Notiz-Ansicht), Spec 0019
(Diff-Darstellung).

## 1. Überblick

Die Historie einer Notiz listet je Revision Zeitpunkt, Bearbeiter (Nutzer
oder KI mit Anbieter und Modell) und „Wiederherstellen". Zusätzlich lässt
sich je Revision die Änderung anzeigen.

## 2. Vergleichsgrundlage

Jede Revision wird mit ihrer **chronologisch unmittelbar vorherigen**
Revision verglichen, mit derselben zeilenbasierten Darstellung wie die
Vorschau von Notiz-Vorschlägen (Spec 0019, Abschnitt 4).

## 3. Verhalten

- Jeder Eintrag ist standardmäßig **eingeklappt**. Ein Klick auf den
  Eintrag klappt den Vergleich auf und wieder zu. Mehrere Einträge können
  gleichzeitig aufgeklappt sein.
- Die **älteste** Revision hat keinen Vorgänger. Aufgeklappt zeigt sie den
  vollen Inhalt unter der Beschriftung „Ursprüngliche Version", ohne
  Diff-Hervorhebung.
- Der Vergleich entsteht aus den Revisionen, die die Historie ohnehin
  geladen hat; dafür ist kein weiterer Abruf nötig.
- Farben wie in Spec 0019: hinzugefügte Zeilen grün, entfernte rot und
  durchgestrichen.

## 4. Grenzen

- Kein Vergleich über mehr als zwei benachbarte Revisionen hinweg.

## 5. Akzeptanzfall

Historie mit mindestens drei Revisionen: Die mittlere zeigt aufgeklappt den
Unterschied zur direkt vorherigen (nicht zur ältesten und nicht zur
aktuellen); die älteste zeigt „Ursprüngliche Version" ohne Diff.
