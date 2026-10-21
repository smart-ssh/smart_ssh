# Spec 0054 — Dateibrowser: Aktionen und Menüs

Status: umgesetzt
Zweck: Welche Aktionen der Dateibrowser dem Nutzer anbietet, wie er sie bestätigt und welche Schutzmechanismen für manuelle, server-verändernde Aktionen gelten, darunter der Ablauf „Lokal öffnen, bearbeiten, Upload anbieten“.
Bezüge: Spec 0020 (Dateibrowser, Freigabe lokaler Pfade), Spec 0053 (Layout), Spec 0058 (Meldungen), Spec 0067 (erhöhter Modus, Erfolgsmeldungen), Spec 0086 (Größengrenzen), ADR 0144 (Ordner-Upload).

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
(Datei oder Ordner); „Dateiinhalt kopieren“, „Datei kopieren“ und „Lokal
öffnen“ gibt es nur für Dateien. Die Reihenfolge ist: lesende und lokale Aktionen, ein Trenner,
dann die server-verändernden Aktionen.

Rechtsklick auf **leere Fläche** der Dateiliste (unter den Zeilen oder in einem
leeren Verzeichnis) öffnet an der Mauszeigerposition ein eigenes Menü mit
„Neuer Ordner“ und „Neue Datei“; das eingebaute Menü des Webviews erscheint
dort nicht. Ist gerade ein Eintrag ausgeschnitten, enthält es zusätzlich
„Einfügen“ (dieselbe Verschiebung wie die Schaltfläche in der Werkzeugleiste),
sonst nicht. Ein Rechtsklick auf eine Zeile öffnet weiterhin das Eintragsmenü.
Klick außerhalb und Escape schließen das Menü; es ist per Tastatur bedienbar
und beschriftet.

## Teil 2: Lesende und lokale Aktionen (ohne Bestätigung)

- **Herunterladen** direkt in den Download-Ordner des Betriebssystems, oder
  **Herunterladen nach…** mit Dialog in einen gewählten Pfad. Ordner werden
  rekursiv heruntergeladen und landen als `<Zielordner>/<Ordnername>/…`.
  Jede Datei erscheint als eigene Übertragung mit Fortschritt.
- **Dateiinhalt kopieren:** Der Text der Datei geht in die Zwischenablage.
  Binärdateien und Dateien über der Grenze aus Spec 0086 (50 MB) werden mit
  einer Meldung abgelehnt; große Dateien zeigen Fortschritt und lassen sich
  abbrechen.
- **Datei kopieren:** Eine einzelne Remote-Datei beliebigen Typs geht als
  Datei in die Zwischenablage; im Dateimanager des Betriebssystems (Finder,
  Explorer, Linux-Dateimanager) lässt sie sich einfügen. Gilt nicht für Ordner
  und nicht für mehrere Einträge. Die Obergrenze ist 50 MB wie bei „Dateiinhalt
  kopieren“; darüber erscheint dieselbe Meldung und nichts wird geladen.
  Fortschritt und Abbrechen wie dort; ein Abbruch hinterlässt keine Datei und
  ändert die Zwischenablage nicht. Die Aktion arbeitet im normalen und im
  erhöhten Kanal. Die Datei wird mit ihrem Namen in einen privaten
  Zwischenordner der App geladen (nur für den eigenen Nutzer lesbar); die
  Zwischenablage verweist darauf. Die App öffnet oder startet die Datei nie.
  Eine neue „Datei kopieren“-Aktion ersetzt die vorige Zwischendatei. Kann die
  Plattform oder Sitzung keine Datei in die Zwischenablage legen, erscheint
  eine Meldung und sonst ändert sich nichts.
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
- **Neuen Ordner anlegen** im aktuellen Verzeichnis, per Werkzeugleiste oder
  über das Menü auf leerer Fläche (Teil 1).
- **Neue Datei anlegen** im aktuellen Verzeichnis, über das Menü auf leerer
  Fläche: Ein Namensdialog (mit eigenem Titel) legt eine leere Datei an. Der
  Name darf nicht leer sein, kein „/“ enthalten und nicht „.“ oder „..“ sein;
  ein ungültiger Name wird im Dialog abgelehnt, ohne den Server zu fragen.
  **Ein vorhandener Name wird nie überschrieben:** Gibt es schon eine Datei
  oder einen Ordner dieses Namens, ändert sich nichts und ein Fehler-Hinweis
  sagt, dass der Name bereits existiert — für „Neue Datei“ und „Neuer Ordner“
  gleich. Fehlende Rechte werden einmal versucht und als „Zugriff verweigert“
  gemeldet, ohne Wiederholung. Im erhöhten Modus laufen beide Aktionen über
  den erhöhten Kanal als gewählter Nutzer und werden wie alle erhöhten
  Änderungen protokolliert. Nach Erfolg wird die Liste aktualisiert und ein
  Erfolgs-Hinweis erscheint.
- **Verschieben:** innerhalb des Servers per Ausschneiden und Einfügen.
  Existiert das Ziel schon, warnt die App. Umbenennen und Verschieben sind
  für den Server dieselbe Operation.
- **Dateien aus der Zwischenablage hochladen:** Liegen Dateien oder Ordner des
  Betriebssystems in der Zwischenablage (kopiert im Finder, Explorer oder
  Linux-Dateimanager, auch die Zwischendatei aus „Datei kopieren“ unter
  ihrem ursprünglichen Namen), bietet das Kontextmenü der leeren Fläche
  „Einfügen“ an. Das Menü prüft das beim Öffnen; enthält die Zwischenablage
  nur Text, ist sie leer oder nicht lesbar, fehlt der Eintrag. Ist gerade ein
  Eintrag ausgeschnitten, bedeutet „Einfügen“ weiterhin Verschieben und die
  Zwischenablage wird nicht gelesen. Nach dem Klick zeigt die App einen
  nativen Dialog mit den Dateien und dem Zielordner; erst nach „Hochladen“
  wird hochgeladen, bei „Abbrechen“ passiert nichts und nichts wird freigegeben.
  Der Upload läuft wie ein Ablegen: Ordner rekursiv, bestehende Ziele mit
  Überschreib-Vorschau bzw. Ordner-Bestätigung, Fortschritt in der
  Übertragungsliste, im erhöhten Kanal als gewählter Nutzer. Fehlt die
  Schreibberechtigung, erscheint einmal die Meldung „Zugriff verweigert“, ohne
  Wiederholung. Hat sich die Zwischenablage zwischen Menü und Klick geleert,
  erscheint nur ein Hinweis.
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
- Zwischendateien von „Datei kopieren“ liegen in einem privaten Ordner der
  App und werden beim Beenden und spätestens beim nächsten Start entfernt,
  auch nach einem Absturz.
- Lokale Pfade aus der Oberfläche werden nur gelesen, wenn sie freigegeben
  sind (Spec 0020, Abschnitt 5.2).

## Grenzen

- Nach dem Beenden der App ist eine kopierte Datei nicht mehr einfügbar, weil
  die Zwischendatei dann entfernt ist (unter Linux hält zudem nur der
  laufende Prozess die Zwischenablage). Unter Wayland steht die Datei über
  XWayland bereit; ein reiner Wayland-Dateimanager sieht sie womöglich nicht.
- Kopien, die eine Version vor Einführung des Aufräumens beim Start liegen
  gelassen hat, räumt die App nicht automatisch weg, weil sie noch zu einer
  laufenden älteren Installation gehören können.
- Der Download-Ordner ist der des Betriebssystems; ein eigener
  Standardordner ist nicht einstellbar.
- Das Lesen von Dateien aus der Zwischenablage hängt vom Betriebssystem ab
  (unter Linux X11 und Wayland, soweit der Dateimanager Dateiverweise
  bereitstellt); ist sie nicht lesbar, fehlt „Einfügen“ ohne Meldung.
- Verschieben per Drag-and-Drop zwischen Ordnern gibt es nicht, nur
  Ausschneiden und Einfügen.
