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

- Die Titelleiste zeigt nur eine Burger-Schaltfläche und den aktuellen Titel: auf den Übersichtsseiten den App-Namen, in einer Sitzung Servername und Verbindungszustand. Reiter und die Navigation der Übersicht erscheinen nicht inline. Die Titelleiste ist dabei 44 px hoch; der Rest der Leiste bleibt ziehbar, die Fenster-Steuerung der Plattform bleibt frei.
- Die Burger-Schaltfläche öffnet ein Menü von der linken Kante, beschriftet als Navigationsmenü. Es enthält Übersicht, Verbinden, Verwalten, Regeln und Einstellungen; darunter den Abschnitt „Offene Sitzungen“ mit jeder Sitzung samt Name, Zustand (verbindet, verbunden, fehlgeschlagen, Warnhinweis bei wartender Bestätigung) und Schließen-Schaltfläche. Jeder Eintrag führt genau dorthin wie sein Gegenstück im breiten Layout; das Schließen einer Sitzung verhält sich wie das Schließen ihres Reiters (gleiche Rückfrage, gleicher Verbindungsabbau) und lässt das Menü offen.
- Der aktuelle Eintrag ist markiert.
- Das Menü schließt bei Auswahl eines Eintrags, mit Escape, bei einem Tipp außerhalb und sobald das Fenster 640 px erreicht. Solange es offen ist, bleibt der Fokus darin; danach kehrt er zur Burger-Schaltfläche zurück.
- Die Tastenkürzel für Sitzungen gelten unverändert.

## 4. Sitzungsansicht

- Eine Sitzung zeigt die Reiter KI, Terminal und Dateien; jeweils einer ist sichtbar.
- Nach dem Verbinden ist der Reiter KI aktiv.

## 5. Dialoge und Zwei-Spalten-Ansichten

- Dialoge füllen das ganze Fenster.
- Ansichten mit zwei Bereichen (Liste und Detail) zeigen zuerst die Liste; die Auswahl öffnet das Detail, und ein Zurück-Element führt zur Liste.

## 6. Erweiterungsbereiche

- Inhalte, die eine Edition in Erweiterungsbereichen einsetzt, haben keine feste Breite. Sie passen sich der Fensterbreite an und brechen um oder kürzen, statt die Seite zu verbreitern.
- Auch ihre primären Bedienelemente erfüllen die Mindestgröße aus Abschnitt 2.

## 7. Grenzen

- Ein Mindestmaß für die Fenstergröße gibt es nicht.
