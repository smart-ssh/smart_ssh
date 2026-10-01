# Spec 0100 — Host-Key-Dialog: modal angesagt, Fokus gefangen, Escape lehnt ab

Status: Vorschlag (Architekt) · Backlog: BL-0232 · Gate: release-1.0/A
Zweck: Die Host-Key-Abfrage (unbekannt und geändert) ist allein mit der Tastatur sicher bedienbar — beim Öffnen liegt der Fokus auf der ablehnenden Schaltfläche, Tab verlässt den Dialog nicht, Escape lehnt ab, und Screenreader sagen ihn als modalen Dialog an.
Review-Priorität: ERHÖHT (Ausführungspfad: Bestätigung einer Verbindung)

## 1. Ist-Stand (Stand `b71d24d`)

**Der Dialog.** `HostKeyDialog`
(`apps/smart-ssh-community/frontend/src/components/HostKeyDialog.tsx`)
rendert per `createPortal` an `document.body` ein Overlay `fixed inset-0`.
Zwei Zweige über `event.kind`: `mismatch` (geänderter Schlüssel, rot,
Schaltflächen „Verbindung abbrechen“ → `reject`, dann „Trotzdem vertrauen“
→ `trust`) und unbekannt („Ablehnen“ → `reject`, dann „Vertrauen“ →
`trust`). Die Überschrift ist ein `<h2>`. Es gibt kein `role`, kein
`aria-*`, keinen `autoFocus`, keinen `ref`, kein `useEffect`, kein
`onKeyDown` (Grep auf diese Muster in der Datei: kein Treffer). Ein Klick
auf den Hintergrund löst nichts aus (kein `onClick` am Overlay).

**Aufrufer.** `ServerList` (Verbindungsaufbau; Entscheidung geht über
`confirmHostKey` an das Backend-Command `confirm_host_key`) und
`ServerForm` (Verbindungstest; `trust` → erneut testen, sonst schließen)
(gelesen, nicht ausgeführt).

**Kein Baustein und kein Test.** Im Frontend gibt es keinen Modal-Baustein
mit Fokus-Fang, kein `aria-modal`, keinen `useFocusTrap`; `role="dialog"`
kommt nur in `FileBrowserPanel` vor. Eine Testdatei für `HostKeyDialog`
gibt es nicht. Testumgebung: Vitest mit `jsdom` und
`@testing-library/react` (`package.json`).

**Linter.** `.oxlintrc.json` aktiviert die Plugins `react`, `typescript`,
`oxc`. Das Plugin `jsx-a11y` ist in der installierten oxlint-Version 1.80.0
verfügbar (Schema enthält den Schlüssel). **Gemessen:** oxlint mit
zusätzlich `jsx-a11y` auf `src` (Konfiguration außerhalb des Repos):
Rückgabewert 0, 15 Funde, alle Warnungen — `prefer-tag-over-role` 5,
`control-has-associated-label` 4, `no-autofocus` 3,
`click-events-have-key-events` 2, `label-has-associated-control` 1. Keiner
davon in `HostKeyDialog.tsx`.

## 2. Teil 0

Teil 0: entfällt — das Verhalten ist rein im Frontend und in `jsdom`
testbar; die Linter-Wirkung ist gemessen.

## 3. Ziel und Nicht-Ziele

Ziel: Die beiden Zweige des Host-Key-Dialogs erfüllen A1–A5; `jsx-a11y`
ist aktiv und verhindert neue mechanische Fälle.

Nicht-Ziele:
- Die 15 Altfunde von `jsx-a11y` werden **nicht** behoben (eigenes Item,
  A6.2).
- Keine Änderung an Texten, Optik, Reihenfolge der Schaltflächen oder am
  Backend (`confirm_host_key`, Timeout).
- Andere Dialoge bekommen in dieser Spec keinen Fokus-Fang. Entsteht dafür
  ein wiederverwendbarer Baustein, darf er von anderen später genutzt
  werden; umgestellt wird hier nur `HostKeyDialog`.
- Portal-Umstellung anderer Dialoge (BL-0162) ist nicht Teil dieser Spec.

## 4. Anforderungen

**A1 Ansage** — MUSS: Das Dialogfenster trägt `role="alertdialog"` im
Zweig „geändert“ und `role="dialog"` im Zweig „unbekannt“, jeweils
`aria-modal="true"`, `aria-labelledby` auf die sichtbare Überschrift und
`aria-describedby` auf den erklärenden Text.

**A2 Anfangsfokus** — MUSS: Beim Öffnen liegt der Fokus auf der
ablehnenden Schaltfläche (`reject`), in beiden Zweigen. Ein Enter direkt
nach dem Öffnen lehnt also ab.

**A3 Fokus-Fang** — MUSS: Tab von der letzten und Shift+Tab von der ersten
fokussierbaren Stelle bleiben im Dialog. Fokus, der auf andere Weise nach
außen gelangt (etwa Klick auf den Hintergrund), wird in den Dialog
zurückgeholt.

**A4 Escape** — MUSS: Escape löst `onDecision({ decision: "reject" })`
genau einmal aus, nie `trust`. Escape wird nicht an dahinterliegende
Handler weitergereicht, solange der Dialog offen ist.

**A5 Keine Entscheidung ohne Absicht** — MUSS: Weder Hintergrund-Klick
noch Fokusverlust noch Schließen des Dialogs auf anderem Weg löst `trust`
aus. Ein Tastendruck, der bereits vor dem Öffnen gedrückt war (Enter
gehalten aus einem vorherigen Dialog, Auto-Repeat), löst keine
Entscheidung aus.

**A6 Linter**
- A6.1 MUSS: `jsx-a11y` ist in `.oxlintrc.json` aktiv, mit
  Standard-Schweregrad (heute Warnung). `npm run lint` bleibt mit
  Rückgabewert 0.
- A6.2 MUSS: Die Altfunde sind im Bericht aufgelistet (Regel, Datei:Zeile),
  damit der Architekt daraus ein Item anlegt.
- A6.3 MUSS: `HostKeyDialog.tsx` hat danach keinen `jsx-a11y`-Fund. Nötige
  Ausnahmen (z. B. `no-autofocus`, falls der Anfangsfokus so gelöst wird)
  werden zeilengenau begründet unterdrückt, nicht global abgeschaltet.

**A7 Fokus zurück** — SOLL: Nach der Entscheidung kehrt der Fokus auf das
Element zurück, das ihn vor dem Öffnen hatte, sofern es noch existiert.

**A8 Changelog** — MUSS: Fragment unter `changelog.d/`.

## 5. Design

- Ob Fokus-Fang und Escape als kleiner wiederverwendbarer Baustein oder
  direkt in `HostKeyDialog` entstehen, entscheidet der Coder. Keine neue
  Abhängigkeit für den Fokus-Fang.
- A5 (gehaltene Taste): Eine Entscheidung per Tastatur zählt nur, wenn das
  zugehörige `keydown` nach dem Öffnen begonnen hat (etwa: `repeat` ist
  falsch, oder Enter wird erst nach einem `keyup` bzw. einer kurzen
  Sperrzeit nach dem Öffnen angenommen). Den Weg wählt der Coder; T6
  legt das Verhalten fest.

## 6. Sicherheits-Invarianten

- **Ein Timeout oder Abbruch lehnt ab, er gewährt nie.** Escape, Fokusverlust,
  Hintergrund-Klick und gehaltene Tasten führen höchstens zu `reject`
  (A4, A5).
- **Die sichere Wahl ist die bequemste.** Anfangsfokus auf `reject` (A2).
- Backend-Pfad `confirm_host_key` und Timeout bleiben unverändert;
  Filter, Redactor und Credentials sind nicht berührt.

## 7. Tests

Alle als Komponententests in `jsdom`, je für beide Zweige
(`mismatch` und unbekannt), wo nicht anders gesagt.

- **T1 Ansage** (A1): Rolle (`alertdialog` bzw. `dialog`), `aria-modal`,
  zugänglicher Name gleich der Überschrift, Beschreibung gesetzt.
  Scheitert ohne die Attribute oder mit vertauschter Rolle.
- **T2 Anfangsfokus** (A2): Nach dem Rendern ist `document.activeElement`
  die `reject`-Schaltfläche. Scheitert, wenn der Fokus auf `trust`, dem
  Body oder dem Overlay liegt.
- **T3 Enter nach dem Öffnen** (A2, adversarial): Ein frisches Enter
  (keydown ohne `repeat`, danach keyup) direkt nach dem Öffnen führt zu
  `reject`, nie zu `trust`.
- **T4 Tab-Fang** (A3, adversarial): Tab von der letzten Schaltfläche führt
  zur ersten, Shift+Tab von der ersten zur letzten; ein fokussierbares
  Element außerhalb des Dialogs (im Test gerendert) wird nie erreicht.
  Scheitert ohne Fang.
- **T5 Escape** (A4, adversarial): Escape ruft `onDecision` genau einmal mit
  `reject`; ein außerhalb registrierter `keydown`-Handler für Escape wird
  nicht ausgelöst. Escape zweimal schnell hintereinander → höchstens ein
  Aufruf. Scheitert, wenn Escape `trust` auslöst oder durchgereicht wird.
- **T6 Gehaltene Taste** (A5, adversarial): Ein Enter-`keydown` mit
  `repeat: true` unmittelbar nach dem Öffnen löst **keine** Entscheidung
  aus. Scheitert, wenn Auto-Repeat eine Wahl trifft.
- **T7 Hintergrund und Fokusverlust** (A3, A5, adversarial): Klick auf das
  Overlay ruft `onDecision` nicht auf; Fokus wird per Programm auf ein
  Element außerhalb gesetzt → er landet wieder im Dialog, kein Aufruf.
- **T8 Bestehendes Verhalten** : Klick auf jede Schaltfläche ruft genau
  die zugehörige Entscheidung (`reject`/`trust`) auf; Texte unverändert.
- **T9 Linter** (A6): `npm run lint` Rückgabewert 0 mit aktivem
  `jsx-a11y`; ein absichtlich eingefügtes `<div onClick>` ohne Tastatur in
  einer Testkopie erzeugt einen `jsx-a11y`-Fund (Gegenbeweis von Hand, in
  den Bericht).
- **T10 Fokus zurück** (A7): Vor dem Öffnen fokussierte Schaltfläche hat
  nach der Entscheidung wieder den Fokus.

Jeder Test muss am heutigen Stand scheitern (außer T8); Beleg im Bericht.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `fix(frontend): keep keyboard focus inside the host key dialog and reject on escape [BL-0232]` — A1–A5, A7, T1–T8, T10.
2. `chore(frontend): enable the jsx-a11y lint plugin [BL-0232]` — A6, T9.
3. `docs(changelog): note the keyboard-safe host key dialog [BL-0232]` — A8.

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Enter aus einem vorherigen Dialog (gehalten oder Auto-Repeat) trifft die
  erst gerade fokussierte Schaltfläche (T6).
- Der Anfangsfokus landet nach einem Re-Render oder bei einem zweiten
  Ereignis doch auf `trust` oder dem Body (T2, Zweigwechsel).
- Escape wird in einem Handler behandelt, der in einer Lage `trust` ruft,
  oder erreicht dahinterliegende Handler (T5).
- Shift+Tab, Klick auf das Overlay oder programmatischer Fokus führen aus
  dem Dialog heraus (T4, T7).
- Der Fokus-Fang greift nicht, weil der Dialog per Portal außerhalb des
  Teilbaums hängt, in dem der Fang lauscht.

**Aufteilung:** ein Lauf auf Sonnet. Reine Frontend-Arbeit an einer
Komponente; der spec-reviewer prüft mit Priorität ERHÖHT.

**Berührte Module:** `HostKeyDialog.tsx` (+ neue Testdatei, ggf. ein
kleiner Baustein unter `src/`), `.oxlintrc.json`, `changelog.d/`.

**Melde zurück:** Beleg, dass T1–T7 und T10 am alten Stand scheitern; die
Liste der `jsx-a11y`-Altfunde (A6.2); Gegenbeweis T9; manueller
Testablauf (Dialog per unbekanntem Host auslösen, nur Tastatur, VoiceOver).
