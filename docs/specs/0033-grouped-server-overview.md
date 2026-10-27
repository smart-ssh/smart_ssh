# Spec 0033 — Gruppierte Server-Übersicht

Status: umgesetzt
Zweck: Die Haupt-Serverübersicht, über die ein Server zum Verbinden gewählt wird, zeigt die Server nach ihren Gruppen gegliedert statt als flache Liste.
Bezüge: Spec 0003 (Gruppen), Spec 0008 (Verwaltungs-Sidebar, Abschnitt 6 und 6a), Spec 0032 (lokaler Pseudo-Server).

## 1. Ziel

Die Gruppierung, die in der Verwaltungs-Sidebar (Bearbeiten-Formular, Spec 0008)
als Baum sichtbar ist, erscheint auch in der Haupt-Übersicht. Server werden nach
ihrer Gruppenzugehörigkeit abschnittsweise dargestellt, nicht als ein großer
Haufen.

## 2. Abgrenzung

Die Haupt-Übersicht ist der Bildschirm, über den ein Server zum Verbinden
gewählt wird; ein Klick auf einen Server verbindet direkt. Die
Verwaltungs-Sidebar öffnet stattdessen das Bearbeiten-Formular. Beide
Ansichten dürfen sich in Darstellung und Interaktion unterscheiden.

## 3. Struktur

- Ein einklappbarer Abschnitt pro Gruppe, verschachtelte Untergruppen
  entsprechend ihrer Tiefe eingerückt. Die Kopfzeile zeigt einen deutlichen
  Pfeil (nach rechts, wenn zugeklappt, nach unten, wenn aufgeklappt; 16 px, im
  schmalen Layout 20 px); die ganze Kopfzeile klappt auf und zu. Server und
  Untergruppen beginnen waagerecht dort, wo das 📁-Symbol ihrer Gruppe beginnt.
  Die Kopfzeile hat kein „+": neue Server legt man in der Verwalten-Ansicht an
  (dort in der gewählten Gruppe) oder über die Aktion des leeren Zustands.
- Ein Abschnitt **„Ohne Gruppe"** für Server ohne Gruppe, am Ende der Liste. Er
  erscheint, sobald mindestens ein Server ungruppiert ist (beim Ziehen eines
  Eintrags auch ohne, als Ablageziel für die oberste Ebene).
- Der lokale Pseudo-Server (Spec 0032) erscheint **fest angepinnt oberhalb**
  aller Gruppen-Abschnitte in einem eigenen abgesetzten Kasten, niemals innerhalb
  eines Ordners und nie unter „Ohne Gruppe".
- Nicht nutzbare Server (Spec 0008, Abschnitt 6a) stehen an ihrem Platz in der
  Gruppe bzw. unter „Ohne Gruppe", als solche markiert und nicht verbindbar.
- Ohne einen einzigen echten Server zeigt die Übersicht zusätzlich einen
  Einstiegs-Block zum Anlegen des ersten Servers; vorhandene Gruppen werden
  daneben trotzdem angezeigt.

## 4. Verhalten

- Ein Klick auf eine Gruppen-Kopfzeile klappt sie ein oder aus. Neu angezeigte
  Gruppen sind aufgeklappt.
- Der Auf-/Zuklappzustand bleibt für die laufende App-Sitzung erhalten, auch
  wenn der Nutzer zwischendurch in einen anderen Bereich (Verwalten,
  Filter-Regeln) wechselt. Über einen Neustart hinweg bleibt er nicht erhalten.
- Leere Gruppen (keine direkten Server, aber ggf. Untergruppen mit Servern)
  werden trotzdem angezeigt, damit die Hierarchie nachvollziehbar bleibt.

## 5. Eine gemeinsame Gruppen-Aufbereitung

Verwaltungs-Sidebar und Haupt-Übersicht bauen den Gruppenbaum aus denselben
Daten mit derselben Regel auf: dieselbe Reihenfolge, dieselbe Zuordnung der
Server zu Gruppen, derselbe Ausschluss des lokalen Pseudo-Servers aus den
Gruppen. Ein Server erscheint in beiden Ansichten in derselben Gruppe.

## 6. Grenzen

- Der Auf-/Zuklappzustand wird nicht dauerhaft gespeichert.
- Es gibt keine Suche und keinen Filter über alle Server hinweg.
