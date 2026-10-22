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
- Nach dem Verbinden ist der Reiter KI aktiv. Die Wahl bleibt, solange die Sitzung offen ist.
- Die Reiter sind per Pfeiltasten bedienbar. Der Reiter Dateien zeigt weiterhin die Markierung des erhöhten Modus.
- Es gibt keinen Trenner. Alle drei Bereiche bleiben geöffnet, auch die nicht sichtbaren; ihr Zustand geht beim Umschalten nicht verloren. Das Terminal passt sich beim Anzeigen der vollen Breite an.
- Beim Wechsel zum breiten Layout gilt wieder die Aufteilung mit der zuletzt gewählten Ansicht des rechten Bereichs; beim Zurückwechseln der zuletzt gewählte Reiter. Die gespeicherte Breite der Aufteilung wird im schmalen Layout weder benutzt noch verändert.
- Eine MCP-Sitzung zeigt nur den Chat, ohne Reiter.
- Der Kopf der Sitzung bleibt kompakt: Servername und Status bleiben sichtbar, „Neu verbinden“ und „Trennen“ liegen hinter einer Schaltfläche „⋯“.

## 5. Dialoge und Zwei-Spalten-Ansichten

- Dialoge füllen das ganze Fenster; Titel und Aktionsleiste bleiben sichtbar, nur der Inhalt dazwischen scrollt. Aktionen brechen um, sodass jede Schaltfläche ganz sichtbar bleibt.
- Die Host-Key-Abfrage folgt denselben Regeln: Fingerabdrücke brechen um und bleiben kopierbar, die Abfrage bleibt modal und lässt sich nicht durch Tippen außerhalb schließen.
- Start- und Entsperransicht passen ohne waagerechtes Scrollen in 390 px Breite. Meldungen (Toasts) nutzen die Fensterbreite abzüglich des Randes; ihre Aktionen bleiben erreichbar.
- Ansichten mit zwei Bereichen (Liste und Detail) zeigen zuerst die Liste; die Auswahl öffnet das Detail, und ein Zurück-Element führt zur Liste.

## 5a. Verbinden-Ansicht

- Serverbaum, Sitzungsauswahl, Erstnutzungs-Hinweis und Verbindungs-Reiter nutzen die volle Breite mit kleinem Rand. Lange Namen, Hosts und Protokollzeilen brechen um, statt die Seite zu verbreitern.
- Zeilen des Serverbaums sind mindestens 44 px hoch. Jede Server- und Ordnerzeile (außer dem lokalen Server) hat eine Schaltfläche „⋯“; sie öffnet „Verschieben nach…“ mit „Ohne Gruppe“ und allen Ordnern. Ziele, die nichts ändern würden, und ein Ordner in seinem eigenen Unterordner sind nicht wählbar. Das Verschieben bewirkt dasselbe wie Ziehen und Ablegen; Ziehen mit der Maus bleibt möglich, eine Fingergeste zum Scrollen startet kein Ziehen.
- Die Sitzungsauswahl ist ein Vollbild-Dialog; die Liste nutzt die verfügbare Höhe, „Neue Unterhaltung“, Umbenennen und Löschen sind mindestens 44 px groß.
- Im Verbindungs-Reiter bleiben „Erneut versuchen“, „Schließen“ und „Abbrechen“ am unteren Rand sichtbar, auch bei langem Protokoll.

## 6. Erweiterungsbereiche

- Inhalte, die eine Edition in Erweiterungsbereichen einsetzt, haben keine feste Breite. Sie passen sich der Fensterbreite an und brechen um oder kürzen, statt die Seite zu verbreitern.
- Auch ihre primären Bedienelemente erfüllen die Mindestgröße aus Abschnitt 2.

## 7. Grenzen

- Ein Mindestmaß für die Fenstergröße gibt es nicht.
