# Spec 0106 — Live-Ausgabe laufender Kommandos im Chat

Status: umgesetzt · Issue: #325
Zweck: Während ein von der KI vorgeschlagenes Kommando läuft, sieht der
Nutzer dessen Ausgabe Zeile für Zeile im Chat, statt bis zum Ende nur einen
Lauf-Indikator. Eine Paketinstallation oder ein langer Kopiervorgang zeigt
den Fortschritt, während er passiert.
Review-Priorität: ERHÖHT (Redaction, Ausführungspfad)

Bezüge: Spec 0005 (Einzelkommando), Spec 0006 (Redaction), Spec 0024
(Texte), Spec 0027 (Abbruch, Lauf-Indikator), Spec 0032 (Localhost),
Spec 0043/0044 (Ausgabegrenze), ADR 0130 (Entscheidungen zu dieser Spec).

## 1. Was sich nicht ändert

Die Live-Ausgabe ist **nur Anzeige**. Unverändert bleiben:

- das ausgeführte Kommando, die Filter-Entscheidung und die Bestätigung;
- das Ergebnis nach dem Ende: dieselbe (begrenzte, redigierte) Ausgabe,
  derselbe Exit-Code, dieselben Hinweise wie ohne diese Spec;
- was die KI bekommt: das Ergebnis nach dem Ende, nach einem Abbruch die
  Teil-Ausgabe mit Abbruch-Kennzeichen (Spec 0027). Die KI liest nie live mit;
- der gespeicherte Verlauf und das Protokoll: nur das Ergebnis. Die
  Live-Ausgabe wird nirgends gespeichert.

## 2. Anzeige

**2.1 Wo.** Sobald ein vorgeschlagenes Shell-Kommando Ausgabe liefert,
erscheint in seinem Block im Chat ein Ausgabebereich. Er gilt für
Kommandos auf einem SSH-Server und auf dem lokalen Pseudo-Server
gleichermaßen. Datei lesen/schreiben (Spec 0020) hat keine Live-Ausgabe.

**2.2 Standardausgabe und Fehlerausgabe** erscheinen getrennt und in
denselben Farben wie im Ergebnis.

**2.3 Mitlaufen.** Der Bereich hat eine begrenzte Höhe und scrollt mit der
neuesten Ausgabe mit, solange der Nutzer darin nicht nach oben gescrollt
hat. Scrollt er wieder ans Ende, läuft der Bereich wieder mit. Die
Live-Ausgabe zieht den übrigen Chat nicht nach unten.

**2.4 Umfang.** Angezeigt wird höchstens das jüngste Stück jeder Ausgabe
(etwa 20 000 Zeichen je Strom), damit die Oberfläche auch bei sehr viel
Ausgabe bedienbar bleibt.

**2.5 Takt.** Je laufendem Kommando kommen höchstens etwa zehn
Aktualisierungen pro Sekunde an.

**2.6 Ende.** Liegt das Ergebnis vor (regulär beendet oder abgebrochen),
ersetzt es den Ausgabebereich. Die Ausgabe erscheint danach nur einmal.
Bricht die Ausführung mit einem Übertragungsfehler ab, bleibt die bis dahin
gezeigte Ausgabe stehen, und die Fehlermeldung erscheint wie bisher.

**2.7 Lauf-Indikator und Abbruch** (Spec 0027) bleiben unverändert und
erscheinen zusätzlich zur Live-Ausgabe. Ein Abbruch während der
Live-Ausgabe wirkt wie bisher.

**2.8 Mehrere Sitzungen.** Jede Sitzung zeigt nur die Ausgabe ihrer eigenen
Kommandos, auch wenn in mehreren Tabs gleichzeitig Kommandos laufen.

## 3. Redaction

**3.1** Die Live-Ausgabe läuft durch denselben Redactor wie das Ergebnis im
Chat (Spec 0006). Ein Geheimnis, das das Ergebnis maskiert, ist auch live
maskiert.

**3.2 Nur ganze, stabile Zeilen.** Weil Geheimnisse über Zeilen reichen
können (Schlüsselblöcke, ein Wert in Anführungszeichen mit Zeilenumbruch,
Schlüssel und Wert auf zwei Zeilen), zeigt die Live-Ausgabe nur ganze
Zeilen, und eine Zeile erst, wenn die nächste Zeile eingetroffen ist und
sich die Maskierung dadurch nicht mehr ändert. Folgen daraus:

- Die jeweils letzte Zeile eines Stroms erscheint live erst, wenn die
  nächste folgt, spätestens mit dem Ergebnis.
- Eine angefangene Zeile ohne Zeilenende erscheint nicht live. Ein
  Wagenrücklauf (Fortschrittsanzeige) zählt als Zeilenende.
- Ein unvollständiger Schlüsselblock wird bis zu seinem Ende
  zurückgehalten.
- Würden neue Zeilen die Maskierung bereits angezeigter Zeilen ändern,
  erscheint statt der neuen Zeilen nur der Platzhalter `[REDACTED]`.
- Bleibt sehr viel Ausgabe ohne stabile Stelle zurückgehalten (mehr als
  256 KB), zeigt dieser Strom bis zum Ende nichts mehr live an.

## 4. Ausgabegrenze

Die Grenze aus Spec 0043/0044 gilt auch live: Ist sie erreicht, zeigt der
Ausgabebereich den Hinweis „Größenlimit erreicht — weitere Ausgabe wird
nicht mehr angezeigt." und keine weitere Ausgabe. Das Kommando wird dabei
genauso behandelt wie bisher, das Ergebnis ist das bisherige begrenzte
Ergebnis.

## 5. Localhost

Auf dem lokalen Pseudo-Server erscheint die Ausgabe genauso live. Abbrechen
lässt sich ein Kommando dort weiterhin nicht (Spec 0027).

## 6. Texte

Neue Texte (Deutsch und Englisch, Spec 0024): Bezeichnung des
Ausgabebereichs für Hilfsmittel („Live-Ausgabe" / „Live output") und der
Hinweis zur Ausgabegrenze aus Abschnitt 4.
