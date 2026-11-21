# Spec 0116 — Aktivitätszentrum

Status: umgesetzt
Zweck: Lang laufende Arbeit, die der Nutzer gestartet hat (z. B. Download eines Modells, Installation einer Komponente, Download eines Updates), läuft im Hintergrund weiter und ist an einer Stelle sichtbar: ein Symbol rechts in der Kopfzeile mit Zähler und ein Popover mit allen Aktivitäten. Das Schließen des Dialogs, der die Arbeit gestartet hat, verliert sie nicht aus den Augen.
Bezüge: Spec 0038 (Erweiterungen), Spec 0108 (Sprachmodelle), Spec 0113 (schmales Layout).

## 1. Symbol in der Kopfzeile

- Ohne Aktivität gibt es kein Symbol.
- Mit Aktivität steht das Symbol am rechten Ende der Kopfzeile, links von den übrigen Bedienelementen, im breiten wie im schmalen Layout.
- Das Symbol ist das Aktivitäts-Symbol. Solange mindestens eine Aktivität läuft, steht daneben die Anzahl der laufenden Aktivitäten in Monospace-Schrift in der Akzentfarbe, und eine dünne Fortschrittslinie am unteren Rand zeigt den zusammengefassten Fortschritt (Summe von erledigt durch Summe von gesamt über alle laufenden Aktivitäten mit bekannter Gesamtgröße). Ist keine Gesamtgröße bekannt, ist die Linie unbestimmt (animiert, ohne Füllstand).
- Laufen keine mehr, aber es gibt beendete Zeilen: Häkchen-Symbol (alles erfolgreich oder abgebrochen) bzw. Warnsymbol (mindestens eine fehlgeschlagen oder unterbrochen).
- Solange das Popover offen ist, trägt das Symbol eine Akzentlinie am oberen Rand.
- Der zugängliche Name nennt den Zustand („Activities: 2 running“, …).

## 2. Popover

- Ein Klick (oder Enter/Leertaste) auf das Symbol öffnet es, ein weiterer Klick, ein Klick außerhalb oder Escape schließt es; Escape gibt den Fokus an das Symbol zurück.
- Das Popover ist 440 px breit (auf schmalen Fenstern so breit wie das Fenster abzüglich Rand) und hat eine Kopfzeile mit dem Titel und einer Zusammenfassung („2 running · 1 interrupted“; nur die vorhandenen Teile).
- Eine Zeile je Aktivität, nach Startzeit geordnet; mehrere Aktivitäten laufen parallel.
- Jede Zeile zeigt ein Symbol der Aktivitätsart, den Titel fett und eine Statusbezeichnung in Großbuchstaben in der Statusfarbe.
- Zeile einer laufenden Aktivität: zusätzlich ein flacher, 6 px hoher Fortschrittsbalken (gestreift, wenn die Gesamtgröße unbekannt ist), darunter eine Monospace-Zeile mit „erledigt / gesamt“ in lesbarer Größe, Geschwindigkeit und Restzeit (Geschwindigkeit und Restzeit nur bei bekannter Gesamtgröße und sobald genug Fortschrittsmeldungen vorliegen), optional ein Statustext (z. B. „Verifying“) anstelle von „In progress“, und darunter eine Symbol-Schaltfläche „Cancel“ (X, mit zugänglichem Namen), wenn die Aktivität abbrechbar ist. Nicht abbrechbare Aktivitäten haben keine.
- „Cancel“ ruft den Abbruch der Aktivität auf; die Zeile endet als „Cancelled“.
- Erfolgreich beendet: die Zeile bleibt mit Häkchen, bis sie verworfen wird („Dismiss“) oder das Popover geschlossen wird, nachdem sie gesehen wurde.
- Fehlgeschlagen: die Zeile bleibt mit der Fehlermeldung, bis sie verworfen wird. Abgebrochen: bleibt bis zum Verwerfen.
- Die Zeilen sind für Screenreader lesbar (Titel, Fortschrittsbalken mit Wert, Status).

## 3. Verhalten über Fenster und Neustart der Oberfläche

- Ein später geöffnetes Fenster oder eine neu aufgebaute Oberfläche zeigt die bereits laufenden Aktivitäten sofort.
- Wer wissen will, ob eine Aktivität einer Art und eines Schlüssels läuft, findet sie im gemeinsamen Zustand; ein erneut geöffneter Dialog zeigt den laufenden Zustand, statt eine zweite Arbeit zu starten.
- Laufende Aktivitäten werden nicht gespeichert: nach einem Neustart der App sind sie weg. Ausnahme: wiederaufnehmbare Aktivitäten (Abschnitt 6), die beim Beenden liefen.

## 4. Aktivitäten starten

- Rust-Code (auch Erweiterungs-Crates) und Frontend-Code (Erweiterungen) können Aktivitäten anmelden; beide verhalten sich gleich: Fortschritt, Statustext, Abbruch-Rückruf, Erfolg, Fehler.
- Frontend-Code kann sich nach einem Neuladen der Oberfläche wieder an eine laufende Aktivität derselben Art und desselben Schlüssels (Schlüssel standardmäßig leer) anhängen und erhält dann einen Besitzer-Handle mit denselben Operationen (Fortschritt, Statustext, Abbruch-Rückruf, Abbruch durch den Besitzer, Erfolg, Fehler); es entsteht keine zweite Aktivität. Läuft keine passende Aktivität (nie gestartet, schon beendet, anderer Schlüssel), gibt es keinen Handle. Laufen mehrere passende, wird die älteste (nach Startzeit) übernommen. Wurde die Aktivität zwischenzeitlich ohne Besitzer abgebrochen, ist sie beendet und wird nicht mehr übernommen; ein Abbruch-Rückruf läuft dafür nicht.
- Nur Frontend-Code kann eine Aktivität zusätzlich selbst abbrechen (Abbruch durch den Besitzer): sie endet als „abgebrochen“ ohne Fehlertext, der Abbruch-Rückruf läuft dafür nicht; bei einer nicht abbrechbaren Aktivität wird der Abbruch abgelehnt und der Fehler an den Aufrufer gegeben.
- Wird die Rust-Seite einer Aktivität verworfen, ohne sie zu beenden, gilt die Aktivität als fehlgeschlagen.
- Fortschrittsmeldungen erreichen die Oberfläche höchstens viermal pro Sekunde je Aktivität; Zustandswechsel (Beginn, neuer Statustext, Ende) kommen immer an.
- Titel, Statustexte und Fehlermeldungen enthalten nie Zugangsdaten oder URLs mit Token.

## 5. Beenden mit laufenden Aktivitäten

- Beenden der App (Menü, Cmd+Q, Schließen des letzten Fensters), während mindestens eine Aktivität läuft und abbrechbar oder wiederaufnehmbar ist, fragt zuerst nach: ein Dialog mit Warnsymbol nennt die laufenden Aktivitäten beim Namen, je mit dem Symbol ihrer Art, und bietet „Trotzdem beenden“ und „Weiterlaufen lassen“.
- „Weiterlaufen lassen“ lässt die App offen und die Aktivitäten laufen. „Trotzdem beenden“ beendet die App.
- Läuft keine solche Aktivität, erscheint kein Dialog. Ein Beenden, das die App selbst auslöst (etwa der Neustart nach einem Update), wird nicht gebremst.

## 6. Wiederaufnehmbare Aktivitäten

- Eine Aktivität kann sich als wiederaufnehmbar anmelden (Art und Schlüssel). Beim bestätigten Beenden werden die laufenden wiederaufnehmbaren Aktivitäten gespeichert: Art, Schlüssel, Titel, erledigte und gesamte Menge – sonst nichts, keine URLs, keine Zugangsdaten.
- Beim nächsten Start erscheinen gespeicherte Einträge, für deren Art ein Handler angemeldet ist (Rust-Seite oder Erweiterung), im Aktivitätszentrum als „Unterbrochen“ (Warnfarbe, zuerst in der Liste) mit „Fortsetzen“ und „Verwerfen“ unter der Zeile. Einträge ohne Handler werden still entfernt.
- „Fortsetzen“ ruft den Handler, der eine neue Aktivität startet; der Eintrag verschwindet. Scheitert das Fortsetzen, bleibt der Eintrag mit einer Fehlermeldung stehen. „Verwerfen“ ruft den optionalen Verwerfen-Handler (etwa Teildateien löschen) und entfernt den Eintrag.
- Gespeicherte Einträge sind je Art und Schlüssel eindeutig: Wird beim erneuten Beenden derselbe Eintrag noch einmal gespeichert, ersetzt der neuere Stand (mit dem jüngeren Fortschritt) den alten; es steht nie dieselbe Aktivität doppelt in der Liste.
- Ein gespeicherter Eintrag, dessen Arbeit inzwischen neu gestartet wurde oder erfolgreich abgeschlossen ist (gleiche Art und gleicher Schlüssel), wird entfernt.
- Sprachmodelle sind wiederaufnehmbar (Spec 0108, 3.9). „Verwerfen“ löscht dort nur die Teildateien eines unterbrochenen Downloads, niemals ein bereits installiertes Modell; „Fortsetzen“ für ein bereits installiertes Modell entfernt den Eintrag, ohne erneut herunterzuladen.

## 7. Texte

Alle Texte (`activities.*`) gibt es in jeder Sprache.
