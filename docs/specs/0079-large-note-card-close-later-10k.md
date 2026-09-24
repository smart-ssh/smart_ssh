# Spec 0079 — Karte „Notiz ist sehr groß“: Schließen, Später, 10 000 Zeichen

Status: **freigegeben** (Stefan, 2026-09-24, Direktauftrag „setz das sofort um“) · Backlog: BL-0260 · Gate: —
Repo: **öffentlich** `smart-ssh` —
`apps/smart-ssh-community/frontend/src/components/NoteShrinkSuggestionToast.tsx`,
`…/components/NotesPanel.tsx`, `…/components/ServerForm.tsx`, `…/api.ts`,
`crates/app-shell/src/orchestration.rs`, `crates/app-shell/src/commands.rs`,
`crates/app-shell/src/lib.rs`, dazu die jeweiligen Tests
Review-Priorität: **NORMAL** (Oberfläche und ein Schwellwert, kein
Sicherheitspfad)
Zweck: Die Karte beim Verbindungsende lässt sich schließen und für die
laufende App-Sitzung zurückstellen, und sie erscheint erst bei wirklich
großen Notizen.

## 1. Ausgangslage

- `NoteShrinkSuggestionToast` zeigt je Server eine Karte „Notiz für Server
  „…“ ist sehr groß“ mit „Mache ich selbst“ und „Ja, zusammenfassen“.
  Einen Weg, die Karte ohne eine der beiden Aktionen loszuwerden, gibt es
  nicht. Die Fehler-Karten derselben Komponente haben bereits einen
  ✕-Knopf (`dismissFailure`).
- Ausgelöst wird die Karte vom Event `note-shrink-suggested`, das
  `suggest_note_shrink_on_disconnect` bei jedem Verbindungsende sendet,
  wenn `note_text.len() >= LARGE_NOTE_DIALOG_THRESHOLD_BYTES` (8 000,
  **Bytes**).
- Dieselbe Konstante erreicht über den Befehl
  `large_note_dialog_threshold_bytes` / `largeNoteDialogThresholdBytes`
  das Frontend. `NotesPanel` und `ServerForm` zeigen damit den
  Hinweis im Notiz-Editor, verglichen gegen die UTF-8-Bytelänge
  (`utf8ByteLength`).

## 2. Ziel und Nicht-Ziele

Ziel:
1. Die Karte hat einen Schließen-Knopf.
2. Die Schwelle ist **10 000 Zeichen** statt 8 000 Bytes, überall, wo die
   Konstante heute wirkt (Karte und Editor-Hinweise), damit Karte und
   Hinweis nicht auseinanderlaufen.
3. Die Karte hat einen Knopf „Später“, danach erscheint sie für diesen
   Server bis zum Neustart der App nicht wieder.

Nicht-Ziele: Persistenz von „Später“ über einen Neustart hinaus (ausdrücklich
nicht gewünscht); Änderungen an `NOTE_SHRINK_MAX_BYTES`, an der
Zusammenfassung selbst oder an der Kompaktierung; i18n dieser Komponente
(sie ist heute fest deutsch beschriftet, das bleibt so).

## 3. Anforderungen

**A1 — Schließen.** Jede Vorschlags-Karte bekommt oben rechts einen Knopf
`✕` mit `aria-label="Hinweis schließen"`, gestaltet wie der ✕-Knopf der
Fehler-Karten. Klick entfernt nur diese Karte (`dismiss`). Beim nächsten
Verbindungsende desselben Servers darf die Karte wieder erscheinen.

**A2 — Später.** Jede Vorschlags-Karte bekommt einen dritten Knopf
„Später“, gestaltet wie „Mache ich selbst“, links davon. Klick entfernt
die Karte und merkt sich die `serverId`. Trifft danach
`note-shrink-suggested` für eine gemerkte `serverId` ein, wird es
ignoriert, es entsteht keine Karte. Andere Server sind nicht betroffen.

**A3 — Speicherort von „Später“.** Die gemerkten Server-IDs liegen nur im
Arbeitsspeicher des Frontends, in einer Variable auf **Modulebene** (nicht
im Komponenten-State, nicht in `localStorage`/`sessionStorage`, nicht im
Backend). So überleben sie ein Neu-Einhängen der Komponente, aber keinen
Neustart der App. Für Tests exportiert die Datei eine Funktion, die die
Menge leert (Name frei, z. B. `resetSnoozedNoteShrinkServersForTests`).

**A4 — Schwelle in Zeichen.** `LARGE_NOTE_DIALOG_THRESHOLD_BYTES` (8 000)
wird zu `LARGE_NOTE_DIALOG_THRESHOLD_CHARS` = **10 000**. Gezählt werden
Unicode-Skalarwerte:
- Backend: `note_text.chars().count()` in
  `suggest_note_shrink_on_disconnect` (Vergleich weiterhin
  `< Schwelle → kein Event`, also ab genau 10 000 Zeichen Karte).
- Frontend: `[...text].length` statt `utf8ByteLength(draft)` in
  `NotesPanel` und statt `new TextEncoder().encode(localNotes).length` in
  `ServerForm` (`isLocalNoteLarge`).
  Ein dann unbenutztes `utf8ByteLength` wird entfernt.

Befehl und API-Funktion werden mit umbenannt:
`large_note_dialog_threshold_bytes` → `large_note_dialog_threshold_chars`
(`commands.rs`, Registrierung in `lib.rs`),
`largeNoteDialogThresholdBytes` → `largeNoteDialogThresholdChars`
(`api.ts`, Aufrufer, Test-Mocks). Das Frontend fragt die Schwelle weiter
beim Backend ab, keine zweite hartkodierte Zahl.

Der Doc-Kommentar der Konstante wird angepasst: Er begründet die Größe
heute mit dem Verhältnis zu `NOTE_SHRINK_MAX_BYTES` („doppelt so groß“).
Neu: Schwelle in Zeichen, bewusst deutlich über der
Zusammenfassungs-Obergrenze, damit normal genutzte Notizen nicht auslösen.
Keine Herkunfts- oder Zeitangaben im Kommentar. Test-Kommentare, die
noch `…_BYTES` oder 8 000 nennen (`NotesPanel.test.tsx`), und die Nennung
der Schwelle in `docs/adr/0050-note-shrink-dialog-design.md` werden
mitgezogen. Dazu ein `CHANGELOG`-Eintrag (sichtbare Änderung).

**A5 — Bestehendes Verhalten bleibt.** „Mache ich selbst“, „Ja,
zusammenfassen“, die Deduplizierung je `serverId` und die Fehler-Karten
verhalten sich unverändert. Klick auf „Ja, zusammenfassen“ oder „Mache ich
selbst“ merkt den Server **nicht** vor.

## 4. Design

Eine Modul-Variable `const snoozedServerIds = new Set<string>()` in
`NoteShrinkSuggestionToast.tsx`. Der Listener von `onNoteShrinkSuggested`
prüft `snoozedServerIds.has(event.serverId)` und kehrt dann früh zurück.
`later(suggestion)` = `snoozedServerIds.add(...)` + `dismiss(...)`.

## 5. Invarianten

- Die gespeicherte Notiz wird durch keinen der neuen Knöpfe verändert.
- Die Schwelle hat genau eine Quelle, die Backend-Konstante.
- Nichts aus A2/A3 wird persistiert.

## 6. Tests

Frontend (Vitest, neue Datei `NoteShrinkSuggestionToast.test.tsx`, Events
über Mocks von `../events`, `beforeEach` leert die Merkmenge):
- T1: Event für Server A → Karte sichtbar; Klick auf „Hinweis schließen“
  → Karte weg; zweites Event für A → Karte wieder sichtbar.
- T2: Event für A → Klick „Später“ → Karte weg; zweites Event für A →
  **keine** Karte; Event für B → Karte für B sichtbar.
- T3: „Später“ überlebt Aushängen und neues Einhängen der Komponente:
  „Später“ für A, `unmount`, neu rendern, Event für A → keine Karte.
- T4: „Mache ich selbst“ merkt nicht vor: Klick, danach Event für A →
  Karte sichtbar.
- Bestehende `NotesPanel`- und `ServerForm`-Tests laufen mit dem
  umbenannten Mock weiter, **außer** `counts UTF-8 bytes, not UTF-16 code
  units` in `NotesPanel.test.tsx`: Er prüft die alte Byte-Zählung und wird
  durch den folgenden Zeichen-Test **ersetzt**.
- Zeichen-Test in `NotesPanel.test.tsx`, Schwelle 10:
  - 5 × „ä“ (10 Bytes, 5 Zeichen) → **kein** Hinweis (scheitert mit der
    alten Byte-Zählung);
  - 6 × „😀“ (12 UTF-16-Einheiten, 6 Zeichen) → **kein** Hinweis (scheitert
    mit `draft.length`);
  - 10 × „ä“ → Hinweis.

Rust (`orchestration.rs`, bestehende Tests der Funktion anpassen):
- T5: 9 999 × `n` → kein Event; 10 000 × `n` → Event.
- T6: 6 000 × `ä` (12 000 Bytes, 6 000 Zeichen) → **kein** Event. Scheitert
  mit der alten Byte-Zählung.
- Der bestehende Test, der `"n".repeat(LARGE_NOTE_DIALOG_THRESHOLD_BYTES)`
  nutzt, wird auf die neue Konstante umgestellt.

Gate: wie in der `CLAUDE.md` des Repos, Rust und Frontend.

## 7. Umsetzungsreihenfolge

Ein Lauf, Sonnet:
1. Backend: Konstante, Zählung, Doc-Kommentar, Befehl umbenennen, T5/T6.
2. Frontend: API umbenennen, Zählung in `NotesPanel`/`ServerForm`,
   Karte mit ✕ und „Später“, T1–T4 und Zeichen-Test.
3. Gate, spec-reviewer (NORMAL).

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

- Spec-Audit: ein bestehender Test widerspricht A4 und wird ersetzt
  (§6); Test mit Zeichen außerhalb der BMP ergänzt; Kommentare, ADR 0050
  und CHANGELOG ausdrücklich genannt.
