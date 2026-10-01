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
davon in `HostKeyDialog.tsx`. Der Kommentar in `community.yml` am Schritt
„Frontend lint“ nennt „aktuell 4“ Warnungen.

**`jsdom` (gemessen, Version 27.0.1, direkt in Node):** Ein Enter-`keydown`/
`keyup` auf einer fokussierten Schaltfläche löst **keinen** Klick aus; ein
Tab-`keydown` bewegt den Fokus **nicht**; `KeyboardEvent.repeat` wird
übernommen; `blur()` setzt den Fokus auf `body`. `@testing-library/user-event`
ist nicht installiert. Der einzige globale Tastatur-Handler ist ein
`keydown`-Listener auf `window` in `App.tsx` (Kürzel, kein Escape)
(gelesen, nicht ausgeführt).

## 2. Teil 0

Teil 0: entfällt — die Grenzen von `jsdom` sind gemessen (§1), die
Tests sind danach zugeschnitten; was nur die echte Webview zeigt, ist
Handtest (Melde zurück).

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
- Portal-Umstellung anderer Dialoge (BL-0162) ist nicht Teil dieser Spec,
  obwohl das Item beide gemeinsam nennt: `HostKeyDialog` hängt bereits per
  Portal an `document.body`, BL-0162 hat keine Gate-Priorität, und der
  Zuschnitt hier soll klein bleiben. Entsteht ein wiederverwendbarer
  Baustein (§5), rendert er selbst per Portal an `document.body`, damit
  BL-0162 ihn übernehmen kann.

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
aus. Ein Enter mit Auto-Repeat (`repeat === true`), etwa gehalten aus einem
vorherigen Dialog, löst keine Entscheidung aus und wird verworfen
(`preventDefault`). Die Leertaste ist ausgenommen: Sie löst eine
Schaltfläche erst beim Loslassen aus, und der Fokus liegt beim Öffnen auf
`reject` — schlimmstenfalls wird also abgelehnt.

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
- A5: Kriterium ist `repeat === true` am Enter-`keydown`, keine Sperrzeit.
  Ein frisches Enter bleibt der eingebauten Bedienung der fokussierten
  Schaltfläche überlassen (kein Nachbau in JS).
- Da `jsdom` Tab nicht ausführt, behandelt der Fokus-Fang Tab und
  Shift+Tab selbst im `keydown` (das ist auch im Browser der übliche Weg).
- Der Fang muss Fokus auf `body` (Hintergrund-Klick, `blur`) ebenso
  zurückholen wie Fokus auf ein Element außerhalb.

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
- **T3 Frisches Enter** (A2, A5, adversarial): Direkt nach dem Öffnen ein
  Enter-`keydown` ohne `repeat` auf das fokussierte Element: Das Ereignis
  ist **nicht** `defaultPrevented`, der Fokus liegt auf `reject`, und
  `onDecision` wurde nicht mit `trust` gerufen. (Die Aktivierung selbst
  führt `jsdom` nicht aus; sie ist Handtest.) Scheitert, wenn das Ereignis
  verschluckt wird oder der Fokus auf `trust` liegt.
- **T4 Tab-Fang** (A3, adversarial): Tab-`keydown` auf der letzten
  Schaltfläche führt den Fokus zur ersten, Shift+Tab auf der ersten zur
  letzten. Scheitert ohne Fang (der Fokus bliebe stehen).
- **T5 Escape** (A4, adversarial): Escape ruft `onDecision` genau einmal mit
  `reject`; ein im Test auf `window` (Bubble-Phase, wie in `App.tsx`)
  registrierter `keydown`-Handler sieht das Escape nicht. Escape zweimal schnell hintereinander → höchstens ein
  Aufruf. Scheitert, wenn Escape `trust` auslöst oder durchgereicht wird.
- **T6 Gehaltene Taste** (A5, adversarial): Ein Enter-`keydown` mit
  `repeat: true` unmittelbar nach dem Öffnen ist `defaultPrevented`, und
  `onDecision` wird nicht gerufen. Scheitert am heutigen Stand (dort wird
  das Ereignis nicht verworfen).
- **T7 Hintergrund und Fokusverlust** (A3, A5, adversarial): (a) Klick auf
  das Overlay ruft `onDecision` nicht auf; (b) Fokus per Programm auf ein
  fokussierbares Element außerhalb → er landet wieder im Dialog; (c)
  `blur()` auf `reject`, Fokus auf `body` → er landet wieder im Dialog. In
  keinem Fall ein Aufruf von `onDecision`.
- **T8 Bestehendes Verhalten** : Klick auf jede Schaltfläche ruft genau
  die zugehörige Entscheidung (`reject`/`trust`) auf; Texte unverändert.
- **T9 Linter** (A6): `npm run lint` Rückgabewert 0 mit aktivem
  `jsx-a11y`; ein absichtlich eingefügtes `<div onClick>` ohne Tastatur in
  einer Testkopie erzeugt einen `jsx-a11y`-Fund (Gegenbeweis von Hand, in
  den Bericht).
- **T10 Fokus zurück** (A7): Vor dem Öffnen fokussierte Schaltfläche hat
  nach der Entscheidung wieder den Fokus.

Jeder Test außer T8 muss am heutigen Stand scheitern; Beleg im Bericht.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `fix(frontend): keep keyboard focus inside the host key dialog and reject on escape [BL-0232]` — A1–A5, A7, T1–T8, T10.
2. `chore(frontend): enable the jsx-a11y lint plugin [BL-0232]` — A6, T9; dabei den Warnungszähler im Kommentar von `community.yml` nachziehen.
3. `docs(changelog): note the keyboard-safe host key dialog [BL-0232]` — A8.

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Enter aus einem vorherigen Dialog (Auto-Repeat) trifft die erst gerade
  fokussierte Schaltfläche (T6).
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
kleiner Baustein unter `src/`), `.oxlintrc.json`,
`.github/workflows/community.yml` (nur Kommentar), `changelog.d/`.

**Melde zurück:** Beleg, dass T1–T7, T9 und T10 am alten Stand scheitern;
die Liste der `jsx-a11y`-Altfunde (A6.2); Gegenbeweis T9; manueller
Testablauf in der echten App (unbekannter und geänderter Host): Enter
direkt nach dem Öffnen lehnt ab, Enter gehalten aus dem vorigen Schritt
entscheidet nichts, Tab/Shift+Tab bleiben im Dialog, Klick auf den
Hintergrund und Escape, VoiceOver sagt den Dialog an.
