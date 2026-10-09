# ADR 0126 — Ordner-Upload im Dateibrowser

Status: akzeptiert
Betrifft: Spec 0054 (Teil 3), Spec 0020 (Abschnitt 5), ADR 0113, Issue #128

## Problem

Der Upload kannte nur einzelne Dateien. Ein Ordner-Upload liest viele
lokale Dateien aus einer einzigen Nutzerfreigabe und schreibt viele
Dateien auf den Server. Das Issue ließ vier Verhalten offen.

## Entscheidung

Der Maintainer hat die vier Empfehlungen des Issues bestätigt:

1. **Konflikte im Ordner:** Eine einzige Bestätigung für den ganzen Ordner,
   die jede Remote-Datei auflistet, die überschrieben würde — ohne
   Diff-Vorschau je Datei. Das ist eine ausdrückliche Ausnahme von der
   Diff-Regel aus Spec 0054 Teil 3 und gilt nur für Ordner-Uploads; der
   Einzeldatei-Upload behält seine Diff-Vorschau. Ohne Bestätigung wird
   nichts überschrieben: Das Backend überschreibt eine bestehende Datei nur,
   wenn ihr Pfad in der bestätigten Liste steht; jede andere bestehende
   Datei bleibt unberührt und erscheint als „übersprungen“.
2. **Symlinks:** Alle Symlinks unterhalb des Ordners werden übersprungen und
   in der Zusammenfassung aufgelistet — auch solche, die innerhalb des
   Ordners bleiben. Kein Link wird je verfolgt. Das ist die strengere
   Variante und entspricht dem rekursiven Löschen und `chmod`.
3. **Teilweises Scheitern:** Der Upload macht mit den übrigen Dateien weiter;
   die Zusammenfassung nennt jeden Fehler.
4. **Auswahl:** Ein eigener Eintrag „Ordner hochladen“ neben „Hochladen“
   (native Dialoge können nicht überall Dateien und Ordner mischen). Der
   Ordner wird wie bei Dateien im Backend-Dialog freigegeben.

## Umsetzung (Begründungen)

- **Freigabeprüfung:** Der Ordner ist eine freigegebene Wurzel (ADR 0113).
  Jede Datei und jeder Unterordner läuft beim Durchlauf durch dieselbe Regel
  wie ein Einzel-Upload (kanonisiert, innerhalb einer Freigabe der Sitzung);
  die Regel liegt an genau einer Stelle und wird auch von einer Momentaufnahme
  der Freigaben genutzt, damit der Durchlauf in einem Blocking-Task laufen
  kann. Direkt vor dem Lesen jeder Datei wird erneut geprüft, weil sie seit
  dem Durchlauf durch einen Link ersetzt worden sein kann.
- **Grenzen:** Maximal 64 Ordnerebenen und 100 000 Einträge. Tiefer
  liegende Ordner werden übersprungen und aufgelistet; ein größerer Ordner
  wird abgelehnt, bevor etwas geschrieben wird. So hält weder ein
  verschachtelter noch ein riesiger Baum den Durchlauf auf.
- **Vorschau und Upload getrennt:** Die Vorschau schreibt nichts und liefert
  die Liste der Überschreibungen. Der Upload plant neu und prüft jedes Ziel
  noch einmal; was zwischen Vorschau und Upload neu entstanden ist, wird
  nicht überschrieben (nicht bestätigt).
- **Bestehende Remote-Ordner** werden wiederverwendet (Zusammenführen); ein
  Zielpfad, an dem eine Datei statt eines Ordners liegt (oder umgekehrt),
  ist ein Fehler für diesen Eintrag. Scheitert der Zielordner selbst,
  passiert nichts.
- **Fortschritt:** Jede Datei bekommt ihr eigenes Übertragungs-Ereignispaar
  wie beim rekursiven Download; am Ende steht eine Zusammenfassung
  (hochgeladen, übersprungen, fehlgeschlagen).
- **Audit:** Jede angelegte Remote-Datei und jeder angelegte Ordner wird bei
  erhöhten Rechten wie bisher einzeln protokolliert.

## Abgewogene Alternativen

- **Diff je Datei:** skaliert nicht und lädt zum blinden Klicken ein.
- **Ordner ablehnen, wenn das Ziel existiert:** sicher, aber ein Ordner
  ließe sich nie durch erneutes Hochladen aktualisieren.
- **Symlinks innerhalb des Ordners folgen:** möglich, aber zusätzliche
  Auslegung (Schleifen, Ziele); kann später folgen.
