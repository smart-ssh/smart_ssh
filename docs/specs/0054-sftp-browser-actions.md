# Spec 0054 — Dateibrowser: Aktionen und Menüs

Status: umgesetzt
Zweck: Welche Aktionen der Dateibrowser dem Nutzer anbietet, wie er sie bestätigt und welche Schutzmechanismen für manuelle, server-verändernde Aktionen gelten, darunter der Ablauf „Lokal öffnen, bearbeiten, Upload anbieten“.
Bezüge: Spec 0020 (Dateibrowser, Freigabe lokaler Pfade), Spec 0053 (Layout), Spec 0058 (Meldungen), Spec 0067 (erhöhter Modus, Erfolgsmeldungen), Spec 0086 (Größengrenzen), ADR 0126 (Ordner-Upload).

## Sicherheitsmodell

**Manuelle Dateibrowser-Aktionen laufen weder durch die Filter-Engine noch
durch die KI-Zweitmeinung.** Die Filter-Engine schützt vor der **KI**, deren
Vorschläge geprüft werden müssen. Löst der **Nutzer selbst** im Browser eine
Aktion aus, ist er der vertrauenswürdige Akteur, wie beim Tippen im
Terminal.

Stattdessen gelten für manuelle Aktionen:

1. **Bestätigung bei gefährlichen oder irreversiblen Aktionen:** Löschen und
   Überschreiben. Harmlose Aktionen (Herunterladen, Kopieren, Umbenennen)
   haben keinen Dialog.
2. **Protokollierbarkeit.** Server-verändernde Aktionen (Löschen,
   Umbenennen, Verschieben, Ordner anlegen, Rechte ändern, Hochladen) sind so
   gebaut, dass sie die Quelle „manuell“ tragen. Im erhöhten Modus
   (Spec 0067) hinterlässt jede solche Aktion eine Protokollzeile mit
   Aktion, Pfad, Erfolg und Zielnutzer, nie mit Dateiinhalt. Ein zentrales
   Audit-Log für den normalen Modus gibt es nicht.
3. **Bestehende Schutzmechanismen bleiben:** die Diff-Vorschau beim
   Überschreiben, die Freigabe lokaler Pfade (Spec 0020, Abschnitt 5.2) und
   die Pfad-Validierung.

## Teil 0: Menü öffnet und schließt zuverlässig

Das Drei-Punkte-Menü hinter jedem Eintrag öffnet sich auf Klick, und ein
Klick außerhalb schließt es. Ein Klick auf das Drei-Punkte-Symbol eines
anderen Eintrags wechselt das Menü zu diesem Eintrag, statt es zu schließen
oder offen zu lassen. Es trägt dieselben Aktionen wie das Kontextmenü.

## Teil 1: Kontextmenü und Drei-Punkte-Menü

Rechtsklick auf einen Eintrag öffnet das Kontextmenü; das Drei-Punkte-Menü am
Zeilenende zeigt **dieselben** Aktionen. Welche erscheinen, hängt vom Typ ab
(Datei oder Ordner); „Dateiinhalt kopieren“ und „Lokal öffnen“ gibt es nur
für Dateien. Die Reihenfolge ist: lesende und lokale Aktionen, ein Trenner,
dann die server-verändernden Aktionen.

## Teil 2: Lesende und lokale Aktionen (ohne Bestätigung)

- **Herunterladen** direkt in den Download-Ordner des Betriebssystems, oder
  **Herunterladen nach…** mit Dialog in einen gewählten Pfad. Ordner werden
  rekursiv heruntergeladen und landen als `<Zielordner>/<Ordnername>/…`.
  Jede Datei erscheint als eigene Übertragung mit Fortschritt.
- **Dateiinhalt kopieren:** Der Text der Datei geht in die Zwischenablage.
  Binärdateien und Dateien über der Grenze aus Spec 0086 werden mit einer
  Meldung abgelehnt.
- **Pfad kopieren:** der Remote-Pfad in die Zwischenablage.
- **Eigenschaften:** Name, Pfad, Typ, Größe, Rechte (symbolisch und
  numerisch), Besitzer, Gruppe und Änderungsdatum.
- **Aktualisieren:** lädt den aktuellen Ordner neu.
- **Lokal öffnen:** siehe Teil 4.

## Teil 3: Server-verändernde Aktionen

Kein Filter- oder KI-Gate; Bestätigung bei Irreversiblem.

- **Rechte bearbeiten (chmod):** Ein Dialog mit Checkbox-Matrix
  (Besitzer/Gruppe/andere × lesen/schreiben/ausführen) und numerischer
  Eingabe. Für Ordner optional rekursiv, deutlich gekennzeichnet; die Rechte
  werden dann auf jeden Eintrag im Baum gesetzt, Dateien und Ordner. Das
  Ergebnis nennt die Zahl der geänderten Einträge.
- **Löschen:** Datei oder Ordner, **Bestätigung immer**. Bei Ordnern zeigt
  der Dialog vorab, wie viele Dateien und Ordner gelöscht werden (Vorschau
  wie beim zweistufigen Löschen eines Servers). Ordner werden von unten nach
  oben geleert und entfernt.
- **Umbenennen:** per Dialog. Existiert der Zielname schon, warnt die App.
- **Neuen Ordner anlegen** im aktuellen Verzeichnis.
- **Verschieben:** innerhalb des Servers per Ausschneiden und Einfügen.
  Existiert das Ziel schon, warnt die App. Umbenennen und Verschieben sind
  für den Server dieselbe Operation.
- **Hochladen:** lokale Dateien in den aktuellen Remote-Ordner, per Dialog
  oder Drag-and-Drop. Überschreibt der Upload eine bestehende Datei, zeigt
  die App eine **Diff-Vorschau** (Spec 0020, Abschnitt 4.2); ist eine Seite
  zu groß oder keine Textdatei, zeigt sie stattdessen einen Größenvergleich.
- **Ordner hochladen:** ein lokaler Ordner samt Unterordnern in den aktuellen
  Remote-Ordner, über den Eintrag „Ordner hochladen“ neben „Hochladen“ oder
  per Drag-and-Drop. Der Inhalt landet in
  `<aktueller Remote-Ordner>/<Ordnername>/`; die Struktur wird nachgebaut,
  ein leerer Ordner entsteht als leerer Remote-Ordner. Der Ordner ist nur
  hochladbar, wenn der Nutzer ihn gewählt oder abgelegt hat (Spec 0020,
  Abschnitt 5.2); jede Datei darunter wird kurz vor dem Lesen gegen diese
  Freigabe geprüft. Ein Pfad, der über `..` oder einen Link aus dem Ordner
  hinausführt, wird nie gelesen.
  - **Konflikte:** Würde der Upload bestehende Remote-Dateien
    überschreiben, zeigt die App **eine** Bestätigung für den ganzen Ordner,
    die jede dieser Dateien auflistet. Es gibt keine Diff-Vorschau je Datei;
    das ist die einzige Ausnahme von der Diff-Regel und gilt nur für
    Ordner-Uploads. Ohne Bestätigung wird nichts überschrieben: Bestehende
    Dateien, die nicht in der bestätigten Liste standen, bleiben unberührt und
    stehen als übersprungen in der Zusammenfassung. Bestehende Remote-Ordner
    werden wiederverwendet. Liegt an einem Zielpfad eine Datei statt eines
    Ordners (oder umgekehrt), schlägt dieser Eintrag fehl. Dateien, die es auf
    dem Server noch nicht gibt, brauchen keine Bestätigung.
  - **Symbolische Links** unterhalb des Ordners werden nie verfolgt: Sie
    werden übersprungen und in der Zusammenfassung genannt. Dasselbe gilt für
    Einträge, die keine normale Datei und kein Ordner sind, für Einträge
    außerhalb der Freigabe und für Ordner tiefer als 64 Ebenen. Ein Ordner mit
    mehr als 100 000 Einträgen wird abgelehnt, bevor etwas geschrieben wird.
  - **Teilweises Scheitern:** Scheitert eine Datei (lokal nicht lesbar,
    Remote-Rechte fehlen), macht der Upload mit den übrigen weiter.
    Scheitert das Anlegen eines Unterordners, werden die Dateien darin nicht
    versucht und als fehlgeschlagen gezählt. Scheitert der Zielordner selbst,
    wird nichts hochgeladen.
  - **Fortschritt und Zusammenfassung:** Jede Datei erscheint als eigene
    Übertragung in der Übertragungsliste. Am Ende zeigt die App die Zahl der
    hochgeladenen, übersprungenen und fehlgeschlagenen Dateien; gab es
    Übersprungenes oder Fehler, nennt eine Liste jeden Eintrag mit Grund.
  - Mit erhöhten Rechten (Spec 0067) wird jede geschriebene Remote-Datei und
    jeder angelegte Ordner einzeln protokolliert, wie beim Einzel-Upload.

## Teil 4: Lokal öffnen, bearbeiten, Upload anbieten

Wie „Edit with…“ in Transfer-Clients:

1. **Herunterladen** in einen kontrollierten temporären Ordner der App,
   getrennt je Sitzung. Datei und Ordner sind nur für den Nutzer lesbar.
   Eine Datei über 50 MB wird abgelehnt (Spec 0086, A1.2).
2. **Öffnen** mit dem Standardprogramm des Betriebssystems oder mit dem
   Programm, das der Nutzer für die Endung festgelegt hat (Teil 5).
3. **Überwachen** der lokalen Datei, solange sie in Bearbeitung ist.
4. **Änderung erkannt** (der Nutzer speichert im Programm): Die App meldet
   „Datei X wurde lokal geändert. Auf den Server hochladen?“.
5. **Bei Ja:** Hochladen mit **Diff-Vorschau** und **Konfliktprüfung**. Hat
   sich die Remote-Datei seit dem Download geändert (Änderungsdatum), warnt die
   App vor dem Überschreiben. Lässt sich der Zustand der Datei nicht
   bestimmen, gilt sie nie stillschweigend als unverändert. Wurde die Datei im
   erhöhten Modus geöffnet, läuft der Upload nur über den erhöhten Kanal
   (Spec 0067, A5).
6. **Sauberes Ende:** Die Überwachung stoppt, wenn der Nutzer den Ablauf
   beendet, die Ansicht schließt oder die Sitzung getrennt wird; die
   temporäre Kopie wird dann entfernt.
   Endet die App unsauber (Absturz, beendeter Prozess), bleibt die Kopie
   nicht dauerhaft liegen: Der nächste Start derselben Installation räumt
   alle liegengebliebenen Bearbeitungskopien weg, bevor eine Verbindung
   geöffnet werden kann. Bearbeitungskopien einer gleichzeitig laufenden
   anderen Installation mit eigenem Datenverzeichnis (z. B. Entwicklungs-
   neben installiertem Build) bleiben dabei unberührt. Symbolische Links im
   Temp-Ordner werden als Link entfernt, ihr Ziel nie. Scheitert das
   Aufräumen eines Eintrags, startet die App trotzdem; das Protokoll nennt
   Pfad und Fehler, nie den Dateiinhalt.

Fehlerfälle (lokales Programm nicht gefunden, Datei schon offen, Upload
scheitert) werden sichtbar gemeldet.

## Teil 5: Standardprogramme je Dateityp

- In den Einstellungen legt der Nutzer je Dateiendung ein Programm fest
  (z. B. `.conf` in einem Code-Editor). Ohne Eintrag gilt das
  Betriebssystem-Standardprogramm.
- Die Einstellung ist rein lokal, ohne Bezug zum Server.

## Invarianten

- Manuelle Aktionen laufen **nicht** durch Filter-Engine oder KI-Prüfung.
- Irreversible Aktionen (Löschen, Überschreiben) verlangen eine Bestätigung.
- Überschreiben beim Upload hat eine Diff-Vorschau und eine Konfliktprüfung;
  die einzige Ausnahme ist die aufgelistete Ordner-Bestätigung (Teil 3).
- Temporäre Bearbeitungskopien liegen in einem kontrollierten Pfad und werden
  spätestens beim nächsten Start entfernt, auch nach einem Absturz.
- Lokale Pfade aus der Oberfläche werden nur gelesen, wenn sie freigegeben
  sind (Spec 0020, Abschnitt 5.2).

## Grenzen

- Kopien, die eine Version vor Einführung des Aufräumens beim Start liegen
  gelassen hat, räumt die App nicht automatisch weg, weil sie noch zu einer
  laufenden älteren Installation gehören können.
- Der Download-Ordner ist der des Betriebssystems; ein eigener
  Standardordner ist nicht einstellbar.
- Verschieben per Drag-and-Drop zwischen Ordnern gibt es nicht, nur
  Ausschneiden und Einfügen.
