# ADR 0070: Karte „Notiz ist sehr groß“ — Schließen/Später/10 000 Zeichen, Entscheidungen bei der Umsetzung

Status: Angenommen
Bezug: docs/specs/0079-large-note-card-close-later-10k.md, ADR 0050
(Ursprungsdesign der Karte)

Spec 0079 gibt einen Schließen-Knopf und einen „Später“-Knopf für die
Notiz-ist-groß-Karte sowie eine Schwelle in Zeichen statt Byte vor. Die
Umsetzung folgt der Spec wörtlich (A1–A5); dieses ADR hält die einzige
bewusst offen gelassene Stelle aus dem `spec-reviewer`-Review fest.

## Bewusst nicht nachgezogen: `docs/specs/0058` nennt weiterhin die alte Konstante

`docs/specs/0058-note-server-ui-politur.md` erwähnt an einer Stelle
`LARGE_NOTE_DIALOG_THRESHOLD_BYTES = 8000` — den Namen und Wert, den Spec
0079 ersetzt. Der `spec-reviewer` hat das als möglichen toten Querverweis
gemeldet, selbst aber als unsicher markiert.

Spec 0079, Abschnitt „Anforderungen“ (A4, letzter Absatz), nennt
ausdrücklich, welche Stellen mitgezogen werden: Test-Kommentare in
`NotesPanel.test.tsx` und die Nennung der Schwelle in
`docs/adr/0050-note-shrink-dialog-design.md`. Ältere Specs sind darin nicht
genannt. Das passt zum in `CLAUDE.md` beschriebenen Rhythmus des
Spec-first-Ablaufs: Eine Spec wird zum Zeitpunkt ihrer Umsetzung committet
und beschreibt danach den Stand zu diesem Zeitpunkt — sie ist kein
lebendes Dokument, das jede spätere Änderung nachträgt. `docs/specs/0058`
bleibt deshalb unverändert; wer die historische Korrektheit aller alten
Specs herstellen will, ist ein eigenes, spec-übergreifendes Aufräum-Item.
