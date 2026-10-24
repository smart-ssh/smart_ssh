# Spec 0053 — Dateimanager-Layout: verstellbare Spalten und Bereiche

Status: umgesetzt
Zweck: Der Nutzer passt das Layout des Dateimanagers und die Aufteilung zwischen KI-Bereich und SSH-/SFTP-Bereich an seine Arbeitsweise an. Beides ist reine Darstellung; es berührt keinen Zugriff auf den Server.
Bezüge: Spec 0020 (Dateibrowser, Abschnitt 5.1), Spec 0054 (Aktionsspalte).

## Teil 1: Verstellbare Spaltenbreiten

Die Dateiliste hat die Spalten Name, Größe, Rechte, Besitzer und
Änderungsdatum sowie eine schmale Aktionsspalte (Drei-Punkte-Menü).

- Zwischen den Spaltenköpfen sitzt ein Ziehgriff. Ziehen ändert die Breite
  der Spalte links davon. Der Mauszeiger zeigt den Griff als
  Spaltengrenze an.
- Jede verstellbare Spalte hat eine Mindestbreite und lässt sich nicht auf
  0 zusammenziehen. Auch eine sehr breit gezogene Nachbarspalte drückt die
  Name-Spalte nie unter ihre eigene Mindestbreite.
- Die **Name-Spalte** hat keine feste Breite und nimmt den Rest ein. Größe,
  Rechte, Besitzer und Änderungsdatum sind verstellbar; die Aktionsspalte ist fest und
  hat keinen Griff.
- **Persistenz:** Die gewählten Breiten bleiben über einen Neustart
  erhalten. Sie gelten global, nicht je Server oder Sitzung. Ein fehlender,
  kaputter oder unplausibler gespeicherter Wert (nicht endlich, nicht
  positiv, absurd groß) wird verworfen und durch den Standardwert ersetzt.

### Besitzer-Spalte und Spalten ein-/ausblenden

- Die Spalte **Besitzer** zeigt `Benutzer:Gruppe`. Ist ein Name unbekannt,
  steht an seiner Stelle die Zahl (uid bzw. gid); ist beides unbekannt, ein
  Strich. Sie ist standardmäßig sichtbar.
- Liefert der Server nur Zahlen (SFTP v3), werden die Namen einmal je
  Sitzung aus `/etc/passwd` und `/etc/group` gelesen — ausschließlich per
  SFTP, nie über einen Shell-Befehl. Sind die Dateien nicht lesbar, bleiben
  die Zahlen stehen; das ist kein Fehler.
- Ein Rechtsklick auf die Kopfzeile öffnet ein Menü mit je einem Häkchen für
  Größe, Rechte, Besitzer und Änderungsdatum. Die Name-Spalte steht dort
  nicht und lässt sich nicht ausblenden. Das Menü bleibt beim Umschalten
  offen und schließt per Klick daneben oder Escape.
- Die Auswahl gilt global und bleibt über einen Neustart erhalten; Unbekanntes
  im gespeicherten Wert wird verworfen. Das Verstellen der Breiten funktioniert
  mit ausgeblendeten Spalten unverändert.

## Teil 2: Verstellbare Bereichsgröße

Zwischen dem KI-Bereich (links) und dem SSH-/SFTP-Bereich (rechts) liegt ein
senkrechter Ziehgriff.

- Ziehen verändert die Breite des rechten Bereichs. Beide Bereiche haben eine
  Mindestbreite und lassen sich nicht auf 0 ziehen.
- **Persistenz** wie bei Teil 1: Die gewählte Breite bleibt über einen
  Neustart erhalten.
- **Kleine Fenster:** Passen beide Mindestbreiten nicht nebeneinander, gilt
  eine Aufteilung, in der der rechte Bereich nie breiter als das halbe Fenster
  und nie breiter als die Standardbreite ist. Ab einer Fensterbreite unter
  640 px gilt stattdessen das schmale Layout (Spec 0113 §4). Die gespeicherte Vorliebe bleibt dabei erhalten; sie
  wird weder durch die Anzeige in einem zu kleinen Fenster noch durch eine
  Ziehgeste dort überschrieben, und die Geste hat in diesem Zustand keine
  Wirkung. Wird das Fenster wieder größer, gilt die gespeicherte Breite.
- Wird das Fenster verkleinert, so dass die gespeicherte Breite dem linken
  Bereich nicht mehr seine Mindestbreite lässt, wird nur die angezeigte
  Breite begrenzt.

## Teil 3: Sortierung per Spaltenkopf

- Die Spaltenköpfe (Name, Größe, Rechte, Änderungsdatum) sind Schaltflächen
  und per Tastatur bedienbar. Ein Klick sortiert die Liste nach dieser
  Spalte aufsteigend; ein zweiter Klick auf dieselbe Spalte kehrt die
  Reihenfolge um, eine andere Spalte beginnt wieder aufsteigend.
- Die aktive Spalte zeigt einen Pfeil (▲ aufsteigend, ▼ absteigend) und
  trägt `aria-sort`; alle anderen Köpfe `aria-sort="none"`.
- **Ordner stehen immer oben**; beide Gruppen werden nach der gewählten
  Spalte sortiert. Name: natürliche Reihenfolge ohne Beachtung der
  Groß-/Kleinschreibung („a2“ vor „a10“). Größe: numerisch (Ordner nach
  Name). Änderungsdatum: nach Zeitstempel, Einträge ohne Zeitstempel zuerst.
  Rechte: nach der Rechte-Zeichenfolge. Bei Gleichstand entscheidet der Name
  aufsteigend.
- Standard ist Name aufsteigend. Die Sortierung gilt für alle Verzeichnisse
  und Sitzungen, bleibt beim Navigieren und über einen Neustart erhalten;
  ein fehlender oder ungültiger gespeicherter Wert wird durch den Standard
  ersetzt.
- Ein Klick auf einen Ziehgriff ändert die Sortierung nie.
- Im schmalen Layout gibt es keine Spaltenköpfe; die Liste folgt dort der
  gespeicherten Sortierung.

## Schmales Layout

- Unter 640 px Fensterbreite zeigt der Dateimanager keine Spalten, sondern zweizeilige Einträge; Spaltenbreiten sind dort nicht verstellbar und die gespeicherten Breiten bleiben unverändert (Spec 0113, Abschnitt 5b).

## Nicht Teil dieser Spec

- Rechte-Bearbeitung (chmod) und andere Aktionen: Spec 0054.
- Die Datei-Logik selbst (Navigation, Upload, Download, Anzeige) bleibt
  unverändert.

## Grenzen

- Ziehgriffe für Spalten und Bereich sind **nicht per Tastatur** bedienbar;
  nur Ziehen mit Maus oder Zeigegerät verändert die Breite.
