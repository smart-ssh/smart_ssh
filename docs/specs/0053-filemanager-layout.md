# Spec 0053 — Dateimanager-Layout: verstellbare Spalten und Bereiche

Status: umgesetzt
Zweck: Der Nutzer passt das Layout des Dateimanagers und die Aufteilung zwischen KI-Bereich und SSH-/SFTP-Bereich an seine Arbeitsweise an. Beides ist reine Darstellung; es berührt keinen Zugriff auf den Server.
Bezüge: Spec 0020 (Dateibrowser, Abschnitt 5.1), Spec 0054 (Aktionsspalte).

## Teil 1: Verstellbare Spaltenbreiten

Die Dateiliste hat die Spalten Name, Größe, Rechte und Änderungsdatum sowie
eine schmale Aktionsspalte (Drei-Punkte-Menü).

- Zwischen den Spaltenköpfen sitzt ein Ziehgriff. Ziehen ändert die Breite
  der Spalte links davon. Der Mauszeiger zeigt den Griff als
  Spaltengrenze an.
- Jede verstellbare Spalte hat eine Mindestbreite und lässt sich nicht auf
  0 zusammenziehen. Auch eine sehr breit gezogene Nachbarspalte drückt die
  Name-Spalte nie unter ihre eigene Mindestbreite.
- Die **Name-Spalte** hat keine feste Breite und nimmt den Rest ein. Größe,
  Rechte und Änderungsdatum sind verstellbar; die Aktionsspalte ist fest und
  hat keinen Griff.
- **Persistenz:** Die gewählten Breiten bleiben über einen Neustart
  erhalten. Sie gelten global, nicht je Server oder Sitzung. Ein fehlender,
  kaputter oder unplausibler gespeicherter Wert (nicht endlich, nicht
  positiv, absurd groß) wird verworfen und durch den Standardwert ersetzt.

## Teil 2: Verstellbare Bereichsgröße

Zwischen dem KI-Bereich (links) und dem SSH-/SFTP-Bereich (rechts) liegt ein
senkrechter Ziehgriff.

- Ziehen verändert die Breite des rechten Bereichs. Beide Bereiche haben eine
  Mindestbreite und lassen sich nicht auf 0 ziehen.
- **Persistenz** wie bei Teil 1: Die gewählte Breite bleibt über einen
  Neustart erhalten.
- **Kleine Fenster:** Passen beide Mindestbreiten nicht nebeneinander, gilt
  die Standardaufteilung. Die gespeicherte Vorliebe bleibt dabei erhalten; sie
  wird weder durch die Anzeige in einem zu kleinen Fenster noch durch eine
  Ziehgeste dort überschrieben, und die Geste hat in diesem Zustand keine
  Wirkung. Wird das Fenster wieder größer, gilt die gespeicherte Breite.
- Wird das Fenster verkleinert, so dass die gespeicherte Breite dem linken
  Bereich nicht mehr seine Mindestbreite lässt, wird nur die angezeigte
  Breite begrenzt.

## Nicht Teil dieser Spec

- Rechte-Bearbeitung (chmod) und andere Aktionen: Spec 0054.
- Die Datei-Logik selbst (Navigation, Upload, Download, Anzeige) bleibt
  unverändert.

## Grenzen

- Ziehgriffe für Spalten und Bereich sind **nicht per Tastatur** bedienbar;
  nur Ziehen mit Maus oder Zeigegerät verändert die Breite.
