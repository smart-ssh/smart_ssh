# ADR 0116 — `jsx-a11y`-Funde als Fehler

Status: akzeptiert
Betrifft: Spec 0100 (A6.1, A6.3), Issue #112

## Kontext

Spec 0100 hat `jsx-a11y` mit Standard-Schweregrad (Warnung) eingeschaltet;
die 17 Altfunde blieben stehen. Issue #112 verlangt: alle Funde beheben, den
Schweregrad auf Fehler heben, Ausnahmen nur zeilengenau. Offen ließ das
Issue, wie der Schweregrad gesetzt wird und wie mit Funden umzugehen ist, die
keine echte Barriere sind.

## Entscheidung

1. **Schweregrad je Regel.** oxlint kennt keinen Schweregrad je Plugin. In
   `.oxlintrc.json` steht deshalb jede `jsx-a11y`-Regel, die das Plugin
   heute aktiviert, ausdrücklich mit `error`. Eine Regel, die eine spätere
   oxlint-Version neu ins Plugin aufnimmt, meldet zunächst nur eine Warnung,
   bis sie in die Liste kommt.
2. **Zwei Regeloptionen statt Unterdrückungen.**
   - `no-autofocus` mit `ignoreNonDOM: true`: Die Regel meldete auch die
     gleichnamige Eigenschaft der Komponente `NotesPanel`, die den Fokus
     selbst per Ref setzt. Für echte DOM-Elemente gilt die Regel weiter.
   - `label-has-associated-control` mit `depth: 3`: Der Text der
     Auswahl-Labels im Serverformular steckt drei Ebenen tief; mit der
     Standardtiefe 2 sah die Regel ihn nicht. Das Label hat einen
     zugänglichen Namen.
3. **Semantische Korrektur, wo sie das Verhalten nicht ändert.** Die
   Einträge der Verwalten-Sidebar sind native Buttons (wie in der
   Server-Liste); damit wählen Enter und Leertaste einen Eintrag aus, was
   vorher nur per Maus ging. Leere Tabellenkopfzellen erhalten einen nur für
   Screenreader sichtbaren, übersetzten Namen; Vorschlagslisten-Optionen ein
   `aria-label` mit ihrem Wert.
4. **Zeilengenau begründete Ausnahmen**, wo das semantische Element das
   Verhalten ändern würde:
   - `role="dialog"` im Fehlerdialog des erhöhten Dateibrowsers: ein natives
     `<dialog>` ist ohne `showModal()`/`open` unsichtbar und bringt eigene
     Stile und Top-Layer-Stapelung mit.
   - `role="status"` am Toast-Container: `<output>` ist ein
     Formularelement, dessen Unterstützung als Live-Region schwankt.
   - `role="separator"` am ziehbaren Trenner der Sitzungsansicht: `<hr>`
     kann den Griff als Kindelement nicht aufnehmen.
   - `autoFocus` an den Passwortfeldern der Start-Dialoge und am
     Umbenennen-Feld der Chat-Auswahl: gewollter Anfangsfokus; ihn per
     Effekt zu setzen, änderte nur die Mechanik, nicht das Verhalten.

## Konsequenzen

Ein neuer `jsx-a11y`-Fund lässt den Lint-Schritt in CI scheitern. Der
Kommentar zum Lint-Schritt im CI-Workflow nennt noch eine veraltete
Warnungszahl; ihn nachzuziehen ist eine Workflow-Änderung außerhalb dieses
Issues.
