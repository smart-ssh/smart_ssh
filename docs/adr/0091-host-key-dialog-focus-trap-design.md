# ADR 0091 — Host-Key-Dialog: Fokus-Fang als Hook, zwei Mechanismen für die Fokus-Rückholung, bewusst nicht behobene Review-Funde

Status: akzeptiert · Spec: `docs/specs/0100-host-key-dialog-keyboard.md`

## 1. Ein Hook ohne eigenes Markup statt eines portalten Dialog-Bausteins

Spec 0100 §3 ließ offen, ob Fokus-Fang und Escape als kleiner
wiederverwendbarer Baustein oder direkt in `HostKeyDialog` entstehen, und
verlangte für den Fall eines Bausteins, dass dieser selbst per Portal an
`document.body` rendert (wegen BL-0162).

**Entscheidung:** `useDialogFocusTrap` ist ein reiner Hook ohne eigenes
Markup — er rendert nichts und portalt folglich auch nichts selbst.
`HostKeyDialog` bleibt die einzige Stelle, die `createPortal` aufruft.

**Warum:** Der wiederverwendbare Teil dieser Spec ist Verhalten
(Anfangsfokus, Tab-Fang, Escape, Fokus-Rückgabe), keine Optik — die beiden
Zweige von `HostKeyDialog` unterscheiden sich stark genug (Warnstreifen,
Farben, Spaltenlayout), dass ein gemeinsamer Markup-Baustein entweder viele
Render-Props bräuchte oder die Abweichungen wieder nach außen durchreichen
müsste. Ein Hook, der nur `onKeyDown` zurückgibt und intern auf
`document`-Ebene lauscht, bleibt unabhängig davon, wohin sein Aufrufer
portalt — BL-0162 kann ihn für jeden künftigen Dialog übernehmen, ohne dass
er seinerseits etwas über Portale wissen muss.

**Preis:** Geht BL-0162 den Weg über einen gemeinsamen Dialog-Rahmen (statt
Verhalten je Komponente einzubinden), bleibt dort trotzdem ein eigener
`createPortal`-Aufruf nötig — der Hook liefert das nicht mit.

## 2. Fokus-Rückholung über zwei Mechanismen auf `document`-Ebene

Gemessen in `jsdom` (Spec 0100 §1, vertieft im Review dieses Schritts):

- Ein `blur()` ohne nächstes Ziel fällt auf `document.body` zurück und
  feuert **nur** ein `focusout` (mit `relatedTarget` `null`) — kein
  `focusin` folgt.
- Ein direkter `.focus()` auf ein Element außerhalb ist eine zweite,
  konkurrierende Fokus-Operation: Ein synchroner Re-Fokus **innerhalb**
  des dazugehörigen `focusout` wird von dieser noch laufenden Operation
  überschrieben (gegengelesen in `jsdom`s Quelle; gilt analog im Browser).
  Der Re-Fokus muss im nachfolgenden `focusin` auf dem neuen Ziel
  passieren, wenn die konkurrierende Operation bereits abgeschlossen ist.

**Entscheidung:** `useDialogFocusTrap` registriert dafür zwei Listener auf
`document` (`focusout` für den ersten Fall, `focusin` für den zweiten),
nicht einen gemeinsamen. Beide sind in Abschnitt 5 des Hooks dokumentiert.

## 3. Escape auf `document`-Ebene statt nur im Container-`onKeyDown`

Review-Fund (Runde 1): Lag Escape ausschließlich im `onKeyDown` des
Dialog-Containers, griff A4 nicht mehr, sobald der Fokus *nicht* im Dialog
lag — etwa weil das fokussierte Element entfernt wurde und der Fokus (ohne
eigenes Ereignis, je nach Engine) auf `document.body` zurückfiel. In dieser
Lage hätte Escape nichts ausgelöst **und** wäre, weil React für
`document.body` keine Fiber findet, bis `window` durchgereicht worden.

**Entscheidung:** Escape wird zusätzlich auf `document` abgefangen, parallel
zu den beiden Fokus-Listenern aus Abschnitt 2, unabhängig davon, wo der
Fokus gerade liegt. Das Container-`onKeyDown` behält nur noch Tab-Fang und
die Enter-Repeat-Sperre (A5) — beide ergeben nur Sinn, solange der Fokus
tatsächlich auf einem Element im Dialog liegt.

Behoben mit Test: `HostKeyDialog.test.tsx`, „A4 (Gegenbeweis): Escape lehnt
auch dann ab, wenn der Fokus auf document.body gefallen ist" — entfernt den
fokussierten Knoten direkt aus dem DOM (das löst in `jsdom` kein `focusout`
aus, wie oben gemessen) und prüft, dass Escape trotzdem ablehnt und keinen
`window`-Handler erreicht. Scheitert am Stand vor diesem Fund.

## 4. Reihenfolge der Effekte in `useDialogFocusTrap`

Die Erfassung des vorher fokussierten Elements (A7) und die Registrierung
der Fokus-Fang-Listener liegen in **einem** Effekt mit `[]`-Abhängigkeiten,
der vor dem Anfangsfokus-Effekt (A2, Abhängigkeit `resetKey`) deklariert
ist. Zwei Gründe:

- Effekte laufen beim Mount in Deklarationsreihenfolge. Läuft der
  Anfangsfokus-Effekt vor der Erfassung, hält `previouslyFocusedRef` bereits
  die eigene `reject`-Schaltfläche statt des Elements, das vor dem Öffnen
  den Fokus trug — A7 gibt dann an der falschen Stelle zurück. Gemessen: Mit
  vertauschter Reihenfolge scheitert der T10-Test zuverlässig (auch ohne
  `StrictMode`).
- Erfassung und Fang-Registrierung in einem Effekt zu belassen erzwingt,
  dass beim Unmount zuerst die Listener abgemeldet und erst danach der
  Fokus zurückgegeben wird — sonst könnte der noch aktive Fang die A7-
  Rückgabe als Fokusverlust werten und sofort zurückholen.

Behoben mit Test: `HostKeyDialog.test.tsx`, „T10 unter React StrictMode:
Fokus-Rückgabe bleibt korrekt" — deckt zusätzlich ab, dass die Reihenfolge
auch den von `main.tsx` tatsächlich genutzten `StrictMode`-Lauf übersteht.
Kein eigenständiger Regressionsbeweis zum vor diesem Schritt committeten
Stand (der bestand den Test ebenfalls), wohl aber zu der oben beschriebenen,
falschen Effekt-Reihenfolge.

## 5. Bewusst nicht behobene Funde des Reviews (Runde 1)

- **Zwei gleichzeitig gemountete Fokus-Fänge würden sich den Fokus
  gegenseitig zurückholen (potenzielle Endlosschleife).** Heute nicht
  erreichbar: Die beiden Aufrufer von `HostKeyDialog` (`ServerList`,
  `ServerForm`) können nicht gleichzeitig einen offenen Dialog haben, weil
  der jeweils andere Tab dafür unmountet sein müsste. Gehört zu BL-0162,
  falls weitere Dialoge `useDialogFocusTrap` übernehmen — dann braucht es
  einen Modal-Stack („nur der oberste Fang ist aktiv"), nicht vorher.
- **Die `focusout`-Rückholung unterscheidet nicht, ob ein Mausklick
  innerhalb des Dialogs auf nicht-fokussierbaren Text (z. B. den
  Fingerprint) fiel oder tatsächlich nach außen ging.** In `jsdom` nicht
  beobachtbar, deshalb nicht automatisiert geprüft. Nur ein Bedien-, kein
  Sicherheitsrisiko — und ausgerechnet an der Stelle, an der der Nutzer den
  Fingerprint zum Vergleich markieren/kopieren könnte. Für den Handtest
  vorgesehen (Fingerprint markieren und kopieren); bei Befund eigenes Item.
- **`FOCUSABLE_SELECTOR` deckt `contenteditable`, `summary`, `iframe` und
  eine von Null abweichende, positive Tab-Reihenfolge nicht ab.** Für diesen
  Dialog mit zwei Schaltflächen ohne diese Elemente ohne Wirkung; Punkt für
  eine Wiederverwendung in BL-0162.
- **Globale Tastatur-Kürzel (`Cmd/Ctrl+W`, `Cmd+1..9` aus `App.tsx`) wirken
  weiter, während der Dialog offen ist — nur Escape wird gestoppt.** Keine
  `trust`-Eskalation möglich, da keines dieser Kürzel eine Entscheidung
  auslöst; aus Spec 0100 nicht verlangt.
- **Ein gehaltenes Enter, das die Lücke zwischen dem Schließen dieses
  Dialogs und dem Öffnen eines möglichen zweiten trifft, kann die dann
  fokussierte äußere Schaltfläche (z. B. „Verbinden") erneut auslösen.**
  Keine Trust-Eskalation am Host-Key-Dialog selbst, ein Nebeneffekt
  außerhalb seines Zuständigkeitsbereichs; nicht Teil von A5.
- **Die 15 bestehenden `jsx-a11y`-Funde in anderen Komponenten (A6.2) sind
  nicht behoben.** Laut Spec 0100 §3 ausdrücklich ein eigenes Item.
