# ADR 0082 — Entscheidungen beim Umbau des Bestätigungswartens (Spec 0088)

Status: akzeptiert · Spec: `docs/specs/0088-confirm-wait-without-panics.md` · Backlog: BL-0231

Spec 0088 ließ mehrere Punkte bewusst offen („Den Weg über den vergifteten
Wert wählt der Coder"). Dieses ADR hält fest, welcher Weg gewählt wurde und
warum — und welche Funde des Reviews bewusst stehen geblieben sind.

## 1. Vergiftete Sperren: `PoisonError::into_inner` statt eines neuen Mutex-Typs

`crate::poison::lock_tolerating_poison` ist eine Zeile:
`mutex.lock().unwrap_or_else(PoisonError::into_inner)`.

Alternativen waren ein `parking_lot::Mutex` (kennt keine Vergiftung) und ein
eigener Wrapper-Typ, der `lock()` gar nicht erst fehlbar anbietet. Beide
wurden verworfen: Der erste ist eine neue Abhängigkeit (Spec-Nicht-Ziel), der
zweite verlangte, jede betroffene Sperre auf einen neuen Typ umzustellen —
viel Fläche für ein Problem, das an drei Stellen auftritt.

Die Toleranz ist **kein** allgemeines Rezept. Sie ist für jeden der drei
Werte einzeln begründet, weil jede einzelne Operation darauf in sich
abgeschlossen ist und es keinen halbfertigen Zwischenzustand gibt, vor dem
eine Vergiftung schützen könnte:

- `Session::pending_action` — `Option<ActionId>`, nur Zuweisung und `is_some`.
- `Session::mcp_origin_flags` — `Vec<bool>`, nur `push` und `clone`. Ein
  fehlendes Flag wird stromabwärts als „MCP-originiert" gewertet, also in die
  sichere Richtung (nicht persistieren).
- `ConfirmationRegistry::pending` — `HashMap`. Ein verlorener Eintrag führt zu
  einem `resolve`-Fehler oder in den Timeout, und der lehnt ab. Ein Eintrag
  kann nicht aus dem Nichts entstehen.

Für einen Wert mit mehrschrittiger Invariante wäre dieselbe Toleranz falsch.
Wer `lock_tolerating_poison` an einer neuen Stelle einsetzt, begründet das
dort erneut.

## 2. Der Empfänger hängt am `Confirm`-Zweig (`PreparedDecision`)

A1.4 verlangt, dass ohne Laufzeitprüfung feststeht, dass im `Confirm`-Zweig
ein Empfänger vorliegt. Der bisherige `Option<Receiver>` neben der
`Decision` ließ sich nicht ohne `expect` auspacken.

Gewählt: ein modulinterner Spiegel des Entscheidungstyps, `PreparedDecision`,
dessen `Confirm`-Variante den Guard trägt. Der `expect` entfällt nicht, weil
er „nicht auslösen kann", sondern weil die Situation, die er abfing, nicht
mehr konstruierbar ist.

Verworfen: ein `unreachable!` im zweiten Zweig (wieder ein Panic-Pfad) und
`Decision` selbst um ein Feld zu erweitern (`Decision` liegt in `core`, ist
serialisierbar und geht in Events — ein Empfänger hat dort nichts zu suchen).

## 3. `cancel_if_current` statt `cancel` im Aufräumen

Der Guard räumt den Registry-Eintrag mit `cancel_if_current(action_id,
generation)` ab, nicht mit `cancel(action_id)`. Heute macht das keinen
Unterschied: `ActionId`s werden nicht wiederverwendet. Die Prüfung kostet
nichts und macht das Aufräumen unabhängig von dieser Zusage — sie kann einen
inzwischen neu registrierten Eintrag nicht mehr entfernen. Der Doc-Kommentar
an `ConfirmationRegistry::cancel` warnt genau vor diesem Fall.

Folge: Das frühere explizite `action_confirmations.cancel(&action_id)` im
Timeout-Zweig ist ersatzlos entfallen. Es galt nur für einen Ausgang; das
`Drop` gilt für alle.

## 4. Das Warten verbraucht den Guard (Review-Fund, Runde 1)

`PendingConfirmation::wait_for_decision` nahm zunächst `&mut self` und war
damit mehrfach aufrufbar — ein zweites Pollen eines aufgelösten
`oneshot::Receiver` lässt tokio panicken. Der Typ ist deshalb in zwei
zerlegt: `PendingConfirmation` (Empfänger + Aufräumen, kein eigener `Drop`)
und `ConfirmationCleanup` (nur Aufräumen, mit `Drop`). `wait_for_decision`
verbraucht `self` und gibt das `ConfirmationCleanup` zurück.

Dass `PendingConfirmation` keinen eigenen `Drop` mehr hat, schwächt nichts
ab: Rusts Drop-Glue läuft rekursiv über die Felder, das enthaltene
`ConfirmationCleanup` fällt also mit. Genau dieser Weg — Abbruch **vor** dem
Warten, während Vorschau oder Zweitmeinung — ist der Fall, den T4 prüft; der
Test blieb beim Umbau unverändert und weiterhin grün, und er ist gegen ein
abgeschaltetes Aufräumen nachweislich rot.

Verworfen: ein `Option<Receiver>` mit `take()` (holt die Laufzeitprüfung
zurück, die A1.4 gerade loswerden wollte) und ein `resolved`-Flag (dasselbe,
nur unsichtbarer).

## 5. Die Lint-Ausnahme für Testcode liegt lokal, nicht in `clippy.toml`

Gemessen: Mit `#![deny(clippy::unwrap_used, clippy::expect_used)]` am Modul
`orchestration` meldet clippy **366** Treffer — es nimmt per
`#[cfg(test)] mod …;` eingebundene Testdateien **nicht** von selbst aus. Der
dafür vorgesehene Schalter ist `allow-unwrap-in-tests` /
`allow-expect-in-tests` in einer `clippy.toml`.

Gewählt wurde stattdessen ein `#[allow(…)]` an jeder der acht
Testmodul-Deklarationen. Grund: Eine `clippy.toml` im Wurzelverzeichnis gilt
workspaceweit. Spec 0088 nennt als Nicht-Ziel ausdrücklich „kein Lint
außerhalb von `orchestration/`" — die Ausnahme soll denselben Radius haben
wie der Lint. Preis sind acht Zeilen Wiederholung; die Begründung steht
einmal im Modulkopf von `orchestration.rs`, dort auch der Hinweis, dass die
Ausnahme an die Moduldeklaration gehört und nie als datei-weites `#![allow]`
(das schaltete den Schutz auch für den Produktivcode derselben Datei ab).

## 6. Bewusst nicht behobene Review-Funde

Alle drei sind Befunde des `spec-reviewer` unter „Zurückstellbar", in beiden
Runden bestätigt. Keiner ist durch diesen Schritt entstanden oder
verschlechtert worden.

- **`orchestration/notes.rs` benutzt weiter `register` + `timeout` + `cancel`
  von Hand** (Notiz-Vorschlag beim Trennen, Notiz-Kürzung). Außerhalb von
  Spec 0088: Deren Ist-Stand (§1) beschreibt ausschließlich `action_exec`.
  Die Stellen setzen `pending_action` nicht, der sichtbare Indikator-Fehler
  entsteht dort also nicht; ein abgebrochener Future ließe einen toten
  Registry-Eintrag stehen, der nichts auslösen kann (der Empfänger fällt mit,
  ein späteres `resolve` schlägt fehl). Sie sind die natürlichen nächsten
  Nutzer von `PendingConfirmation` — Backlog.
- **`compaction.rs` enthält weiter ein `expect`** (`group_mcp_flags_by_round`).
  Spec-Nicht-Ziel: „Kein workspaceweiter Abbau von `unwrap`/`expect` und kein
  Lint außerhalb von `orchestration/`."
- **`Session::pending_action` ist ein Einzel-Slot.** Warten Chat und MCP
  gleichzeitig auf derselben Sitzung, überschreibt die zweite Aktion den
  Indikator der ersten, und deren Anzeige ist verloren. Vorbestehend; der
  Guard macht es eher besser (er löscht den fremden Indikator nicht mehr).
  Ein `HashSet<ActionId>` wäre die saubere Lösung — eigenes Item.

Ebenfalls stehen geblieben, mit derselben Begründung wie in Runde 1: das
schmale Rennen im `Drop` zwischen „Indikator löschen" und
`cancel_if_current`. Ein gleichzeitiges `resolve` kann dort `Ok` melden,
obwohl kein Wartender mehr existiert; die Folge ist Nicht-Ausführung, also
fail-safe.
