# Spec 0116 — Aktivitätszentrum

Status: umgesetzt
Zweck: Lang laufende Arbeit, die der Nutzer gestartet hat (z. B. Download eines Modells, Installation einer Komponente, Download eines Updates), läuft im Hintergrund weiter und ist an einer Stelle sichtbar: ein Symbol rechts in der Kopfzeile mit Zähler und ein Popover mit allen Aktivitäten. Das Schließen des Dialogs, der die Arbeit gestartet hat, verliert sie nicht aus den Augen.
Bezüge: Spec 0038 (Erweiterungen), Spec 0108 (Sprachmodelle), Spec 0113 (schmales Layout).

## 1. Symbol in der Kopfzeile

- Ohne Aktivität gibt es kein Symbol.
- Mit Aktivität steht das Symbol am rechten Ende der Kopfzeile, links von den übrigen Bedienelementen, im breiten wie im schmalen Layout.
- Solange mindestens eine Aktivität läuft, zeigt das Symbol die Anzahl der laufenden Aktivitäten und einen Fortschrittsring für den zusammengefassten Fortschritt (Summe von erledigt durch Summe von gesamt über alle laufenden Aktivitäten mit bekannter Gesamtgröße). Ist keine Gesamtgröße bekannt, dreht sich der Ring unbestimmt.
- Laufen keine mehr, aber es gibt beendete Zeilen: Häkchen (alles erfolgreich oder abgebrochen) bzw. Ausrufezeichen (mindestens eine fehlgeschlagen).
- Der zugängliche Name nennt den Zustand („Activities: 2 running“, …).

## 2. Popover

- Ein Klick (oder Enter/Leertaste) auf das Symbol öffnet es, ein weiterer Klick, ein Klick außerhalb oder Escape schließt es; Escape gibt den Fokus an das Symbol zurück.
- Eine Zeile je Aktivität, nach Startzeit geordnet; mehrere Aktivitäten laufen parallel.
- Zeile einer laufenden Aktivität: Titel, Fortschrittsbalken, „erledigt / gesamt“ in lesbarer Größe (unbestimmter Balken, wenn die Gesamtgröße unbekannt ist), optional ein Statustext (z. B. „Verifying“), und eine Schaltfläche „Cancel“, wenn die Aktivität abbrechbar ist. Nicht abbrechbare Aktivitäten haben keine.
- „Cancel“ ruft den Abbruch der Aktivität auf; die Zeile endet als „Cancelled“.
- Erfolgreich beendet: die Zeile bleibt mit Häkchen, bis sie verworfen wird („Dismiss“) oder das Popover geschlossen wird, nachdem sie gesehen wurde.
- Fehlgeschlagen: die Zeile bleibt mit der Fehlermeldung, bis sie verworfen wird. Abgebrochen: bleibt bis zum Verwerfen.
- Die Zeilen sind für Screenreader lesbar (Titel, Fortschrittsbalken mit Wert, Status).

## 3. Verhalten über Fenster und Neustart der Oberfläche

- Ein später geöffnetes Fenster oder eine neu aufgebaute Oberfläche zeigt die bereits laufenden Aktivitäten sofort.
- Wer wissen will, ob eine Aktivität einer Art und eines Schlüssels läuft, findet sie im gemeinsamen Zustand; ein erneut geöffneter Dialog zeigt den laufenden Zustand, statt eine zweite Arbeit zu starten.
- Aktivitäten werden nicht gespeichert: nach einem Neustart der App sind sie weg.

## 4. Aktivitäten starten

- Rust-Code (auch Erweiterungs-Crates) und Frontend-Code (Erweiterungen) können Aktivitäten anmelden; beide verhalten sich gleich: Fortschritt, Statustext, Abbruch-Rückruf, Erfolg, Fehler.
- Wird die Rust-Seite einer Aktivität verworfen, ohne sie zu beenden, gilt die Aktivität als fehlgeschlagen.
- Fortschrittsmeldungen erreichen die Oberfläche höchstens viermal pro Sekunde je Aktivität; Zustandswechsel (Beginn, neuer Statustext, Ende) kommen immer an.
- Titel, Statustexte und Fehlermeldungen enthalten nie Zugangsdaten oder URLs mit Token.

## 5. Texte

Alle Texte (`activities.*`) gibt es in jeder Sprache.
