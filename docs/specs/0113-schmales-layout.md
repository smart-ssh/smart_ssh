# Spec 0113 — Schmales Layout

Status: umgesetzt (Grundlage); die Anordnung je Bereich folgt in eigenen Änderungen
Zweck: Ab einer bestimmten Fensterbreite ordnet die App ihre Oberfläche wie auf einem Telefon an. Die Funktionen bleiben dieselben.
Bezüge: Spec 0052 (Titelleiste), docs/ui-guidelines.md.

## 1. Schwelle

- Das Layout ist „schmal“, solange das Fenster schmaler als 640 px ist. Ab 640 px gilt das bisherige (breite) Layout.
- Der Wechsel geschieht live beim Ändern der Fenstergröße, ohne Neuladen und ohne Verlust von Eingaben oder offenen Sitzungen.
- Die Grundlage selbst ändert in keiner Fenstergröße etwas Sichtbares.

## 2. Grundsatz

- Das schmale Layout bietet dieselben Funktionen wie das breite. Es ändert nur die Anordnung, nichts wird weggelassen, nichts kommt hinzu.
- Die Seite scrollt nie waagerecht. Nur Inhalte wie Code, Tabellen oder das Terminal dürfen innen seitlich scrollen.
- Primäre Bedienelemente sind mindestens 44×44 px groß.

## 3. Rahmen

- Die Navigation liegt hinter einem Burger-Menü. Es enthält Übersicht, Verbinden, Verwalten, Regeln, Einstellungen und die offenen Sitzungen.

## 4. Sitzungsansicht

- Eine Sitzung zeigt die Reiter KI, Terminal und Dateien; jeweils einer ist sichtbar.
- Nach dem Verbinden ist der Reiter KI aktiv.

## 5. Dialoge und Zwei-Spalten-Ansichten

- Dialoge füllen das ganze Fenster; Titel und Aktionsleiste bleiben sichtbar, nur der Inhalt dazwischen scrollt. Aktionen brechen um, sodass jede Schaltfläche ganz sichtbar bleibt.
- Die Host-Key-Abfrage folgt denselben Regeln: Fingerabdrücke brechen um und bleiben kopierbar, die Abfrage bleibt modal und lässt sich nicht durch Tippen außerhalb schließen.
- Start- und Entsperransicht passen ohne waagerechtes Scrollen in 390 px Breite. Meldungen (Toasts) nutzen die Fensterbreite abzüglich des Randes; ihre Aktionen bleiben erreichbar.
- Ansichten mit zwei Bereichen (Liste und Detail) zeigen zuerst die Liste; die Auswahl öffnet das Detail, und ein Zurück-Element führt zur Liste.

## 6. Erweiterungsbereiche

- Inhalte, die eine Edition in Erweiterungsbereichen einsetzt, haben keine feste Breite. Sie passen sich der Fensterbreite an und brechen um oder kürzen, statt die Seite zu verbreitern.
- Auch ihre primären Bedienelemente erfüllen die Mindestgröße aus Abschnitt 2.

## 7. Grenzen

- Ein Mindestmaß für die Fenstergröße gibt es nicht.
