# Spec 0014 — Individuelle Titelleiste

Status: umgesetzt
Zweck: Die native Titelleiste ist durch eine eigene, ins App-Design integrierte Kopfzeile ersetzt, auf allen drei Plattformen. Die nativen Fenster-Controls bleiben erhalten, und das Fenster lässt sich weiterhin an der Titelleiste bewegen, auch bei offenem Dialog und vor der Entsperrung.
Bezüge: Spec 0017 (Session-Tabs in der Titelleiste), Spec 0049 (Fenster-Steuerung auf Windows und Linux), Spec 0052 (Versionsanzeige), Spec 0101 (Startzustand und Entsperrung), ADR 0019 (Titelleisten-Plugin), ADR 0040 (Rückfall auf die native Titelleiste), ADR 0118 (Drag-Schicht über modalen Dialogen).

## 1. Ziel

Die Kopfzeile des Fensters gehört der App: App-Name und Logo sowie die Tab-Leiste
offener Sitzungen (Spec 0017). Die **nativen Fenster-Controls bleiben
erhalten** (macOS-Ampel, Windows-Minimieren/Maximieren/Schließen samt
Snap-Layout-Unterstützung, Linux-Fenster-Controls im System-Icon-Theme). Es
gibt keine eigenen Nachbauten dieser Buttons, nur der Rest der Titelleiste ist
eigener Inhalt.

## 2. Plattformverhalten

- **macOS:** Die native Ampel sitzt oben links, an die Kopfzeile angepasst
  positioniert.
- **Windows:** Minimieren, Maximieren (Wiederherstellen) und Schließen sitzen
  oben rechts über der ziehbaren Titelleistenfläche; das Windows-11-Snap-Layout
  beim Überfahren von Maximieren funktioniert.
- **Linux:** Die Fenster-Controls sitzen oben rechts und nutzen das aktuelle
  System-Icon-Theme.

Fenster-Controls liegen immer über jedem Dialog und bleiben bedienbar.

## 3. Aktivierung und Rückfall

Die eigene Titelleiste wird beim Start des Fensters aktiviert. Die Aktivierung
läuft auch vor der Entsperrung (Spec 0101), weil die Entsperrmaske denselben
Fensterrahmen braucht wie die App.

Scheitert die Aktivierung, stellt die App die native Titelleiste samt deren
Controls wieder her und protokolliert den Fehler; das Fenster bleibt nicht in
einem halb dekorierten Zustand. In diesem Fall braucht die Kopfzeile keinen
Freiraum für Controls mehr. Scheitert dagegen nur die Feinpositionierung der
macOS-Ampel, bleibt die eigene Titelleiste aktiv (ADR 0040).

## 4. Eigener Header-Inhalt und Layout

Die Kopfzeile zeigt links das App-Icon und den Schriftzug „Smart SSH"
(Versions- und Editionszusatz: Spec 0052), in der Mitte Platz für die Tab-Leiste
(Spec 0017).

- **macOS:** Die Ampel sitzt oben links, der Header-Inhalt beginnt mit
  entsprechendem Abstand links.
- **Windows/Linux:** Die Controls sitzen oben rechts, der Header-Inhalt lässt
  rechts entsprechend Platz.
- Die Plattform wird zur Laufzeit erkannt; fällt die Erkennung aus, schätzt die
  App sie aus der Browser-Kennung. Solange die Aktivierung der Titelleiste noch
  aussteht, gilt derselbe Freiraum wie im Normalfall, damit das Layout nicht
  springt.

## 5. Drag-Region und Interaktivität

Der gesamte Header-Bereich außer den nativen Controls ist ziehbar, damit sich
das Fenster wie gewohnt an der Titelleiste bewegen lässt. Interaktive Elemente
im Header, etwa die Session-Tabs, sind von der Drag-Region ausgenommen, sodass
ein Klick darauf ein Klick bleibt und kein Fenster-Ziehen auslöst.

**Bei geöffnetem modalem Dialog.** Ein modaler Dialog dunkelt das ganze
Fenster ab, die Titelleiste eingeschlossen. Die Fensterfunktionen der
Titelleiste bleiben dabei trotzdem erhalten, für jeden modalen Dialog
gleichermaßen, auch für über Erweiterungen beigesteuerte:

- Das Fenster lässt sich weiterhin an der Titelleiste ziehen.
- Ein Doppelklick auf die Titelleiste zoomt bzw. maximiert das Fenster
  wie ohne Dialog, so wie es die Plattform tut.
- Die Fenster-Controls bleiben bedienbar: die Ampel auf macOS,
  Minimieren/Maximieren/Schließen auf Windows und Linux.
- Die Inhalte des Headers (z. B. Session-Tabs) reagieren dagegen nicht
  auf Klicks, solange der Dialog offen ist; nach dem Schließen wieder wie
  gewohnt. Der Dialog bleibt modal und behält seine Fokusführung, die
  Titelleiste trägt währenddessen nur Fensterfunktionen, keine
  App-Inhalte.
- Optisch ändert sich nichts: Die Titelleiste ist so abgedunkelt wie der
  Rest des Fensters.

Als geöffnet zählt nur ein Dialog, der gerade sichtbar ist. Bleibt ein
Dialog in einem Session-Tab oder in der Startansicht offen und wechselt der
Nutzer (etwa per Tastenkürzel) zu einer anderen Ansicht, ist der Dialog dort
nicht zu sehen. Dann reagieren die Header-Inhalte wieder wie ohne Dialog.
Kehrt der Nutzer zurück, gilt wieder das Verhalten bei geöffnetem Dialog.

Ohne geöffneten Dialog verhalten sich Header, Drag-Region und
Fenster-Controls unverändert wie oben beschrieben.

**Auf den Startmasken vor der Entsperrung.** Solange die App noch gesperrt
ist (Entsperrmaske, unbrauchbare oder nicht erreichbare
Verpackungsdatei, unbekannter Startzustand, Fortsetzung des Starts im
Fenster; Spec 0101), gibt es keinen Header. Trotzdem lässt sich das
Fenster an seiner Oberkante bewegen:

- Ein Streifen in Titelleistenhöhe an der Fensteroberkante ist ziehbar;
  ein Doppelklick darauf zoomt bzw. maximiert das Fenster wie auf der
  Titelleiste.
- Der Streifen lässt die Fenster-Controls frei, mit denselben Abständen
  wie der Header (Abschnitt 4): links die Ampel auf macOS, rechts
  Minimieren/Maximieren/Schließen auf Windows und Linux. Die Controls
  bleiben bedienbar.
- Der Streifen ist unsichtbar und trägt keine Inhalte. Die Maske und ein
  geöffneter Startdialog halten oben die Titelleistenhöhe frei, sodass
  keine Eingabe und kein Knopf unter dem Streifen liegt.
- Das gilt auch, während ein Startdialog offen ist: Das Fenster bleibt
  ziehbar, die Fenster-Controls bleiben bedienbar.
- Um den Streifen anzuzeigen, ruft die gesperrte App kein Kommando auf,
  das vor der Entsperrung gesperrt ist (Spec 0101).

Nach der Entsperrung übernimmt der Header; es gibt dann keinen zweiten
Streifen.

## 6. Grenzen

- Der Abstand der macOS-Ampel zum Fensterrand ist ein fest eingestellter
  Startwert und visuell nicht final abgestimmt.
- Eine native macOS-Menüleiste am Bildschirmrand ist nicht Teil dieser Spec.
