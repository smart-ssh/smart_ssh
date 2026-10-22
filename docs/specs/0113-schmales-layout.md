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
- Nach dem Verbinden ist der Reiter KI aktiv. Die Wahl bleibt, solange die Sitzung offen ist.
- Die Reiter sind per Pfeiltasten bedienbar. Der Reiter Dateien zeigt weiterhin die Markierung des erhöhten Modus.
- Es gibt keinen Trenner. Alle drei Bereiche bleiben geöffnet, auch die nicht sichtbaren; ihr Zustand geht beim Umschalten nicht verloren. Das Terminal passt sich beim Anzeigen der vollen Breite an.
- Beim Wechsel zum breiten Layout gilt wieder die Aufteilung mit der zuletzt gewählten Ansicht des rechten Bereichs; beim Zurückwechseln der zuletzt gewählte Reiter. Die gespeicherte Breite der Aufteilung wird im schmalen Layout weder benutzt noch verändert.
- Eine MCP-Sitzung zeigt nur den Chat, ohne Reiter.
- Der Kopf der Sitzung bleibt kompakt: Servername und Status bleiben sichtbar, „Neu verbinden“ und „Trennen“ liegen hinter einer Schaltfläche „⋯“.

## 5. Dialoge und Zwei-Spalten-Ansichten

- Dialoge füllen das ganze Fenster.
- Ansichten mit zwei Bereichen (Liste und Detail) zeigen zuerst die Liste; die Auswahl öffnet das Detail, und ein Zurück-Element führt zur Liste.

## 6. Erweiterungsbereiche

- Inhalte, die eine Edition in Erweiterungsbereichen einsetzt, haben keine feste Breite. Sie passen sich der Fensterbreite an und brechen um oder kürzen, statt die Seite zu verbreitern.
- Auch ihre primären Bedienelemente erfüllen die Mindestgröße aus Abschnitt 2.

## 7. Grenzen

- Ein Mindestmaß für die Fenstergröße gibt es nicht.
