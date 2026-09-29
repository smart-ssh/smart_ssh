# Spec 0088 — Confirm wait and file actions without panic paths

Status: freigegeben · Backlog: BL-0231 · Gate: release-1.0/C
Zweck: Der Tab-Indikator „wartet auf Bestätigung" und der Bestätigungseintrag
werden auf jedem Weg aus dem Warten abgeräumt, und der Orchestrierungscode
enthält keinen `unwrap`/`expect` mehr, den ein neuer Aufrufer unbemerkt erreichen kann.
Review-Priorität: ERHÖHT

## 1. Ist-Stand (Stand `33d0501`; bis `51a3a9d` unverändert für die hier genannten Dateien)

Der betroffene Code liegt seit Spec 0084 in
`crates/app-logic/src/orchestration/`.

**Bestätigungswarten** (`action_exec.rs`, `handle_action_proposed`, Zweig
`Decision::Confirm`; gelesen, nicht ausgeführt):
- `confirm_rx` wird vor den Vorschau- und Zweitmeinungs-`await`s registriert
  (`action_confirmations.register(action_id)`), als `Option` gehalten und
  erst im `Confirm`-Zweig per `expect("confirm_rx muss registriert sein")`
  entpackt.
- Unmittelbar davor `*session.pending_action.lock().unwrap() = Some(action_id)`,
  nach dem `timeout(PENDING_ACTION_CONFIRM_TIMEOUT, rx).await` von Hand
  `… = None`.
- Zwischen Registrierung und `match decision` wird `decision` nicht mehr
  verändert und es gibt kein frühes `return` (gelesen). **Der `expect` kann
  heute nicht auslösen.** Er ist eine Invariante, die nur im Kommentar steht.
- Wird der Future während des Wartens fallen gelassen, bleibt
  `pending_action` auf `Some` und der Eintrag in `ConfirmationRegistry`
  bestehen. Heute bricht kein Produktivpfad diesen Future ab: Der Chat-Turn
  wartet außerhalb jedes `select!` (`chat_turn.rs`, `run_one_round`), der
  MCP-Pfad startet `propose_action` bewusst als eigene Task
  (`mcp-server/src/tool_server.rs`, `run_confirmable`, Doc-Kommentar zu
  Spec 0028 §7), und `grep -rn 'abort()' crates/app-logic/src crates/app-shell/src`
  findet keinen Treffer außerhalb von Tests.
- Leser des Indikators: `SessionManager::snapshot` in `session.rs`
  (`has_pending_action: session.pending_action.lock().unwrap().is_some()`).

**Vergiftete Sperren.** `pending_action` ist ein `std::sync::Mutex`. Ist er
vergiftet, panicken der Snapshot für die Tab-Leiste und jeder weitere
Bestätigungsversuch dieser Sitzung. Alle Stellen, die ihn heute sperren,
enthalten nur eine Zuweisung oder ein `is_some()`. Eine Vergiftung aus
Produktivcode ist damit praktisch ausgeschlossen, von außen aber möglich
(das Feld ist `pub`).

**SFTP-Dateiaktionen** (`remote_files.rs`): sechsmal
`guard.sftp().expect("ensure_sftp_open lief erfolgreich durch")`, jeweils
nach einem erfolgreichen `ensure_sftp_open` und einem **neuen**
`lock_sftp()`. Seit Spec 0085 ist der normale Kanal ein eigener Typ
(`NormalSftpChannel`/`NormalSftpGuard` in `session.rs`). Er wird nur über
`install` befüllt und hat keine Operation, die ihn wieder leert. Der
`expect` kann heute also ebenfalls nicht auslösen. Das garantiert aber der
Typ des Guards, nicht der Aufrufer, und `sftp()` liefert weiterhin `Option`.

**Zählung** (Befehl in der Beilage, negativ getestet): Im Produktivcode von
`orchestration/` stehen genau **10** `unwrap`/`expect`: die drei oben in
`action_exec.rs`, `mcp_origin_flags.lock().unwrap()` in `chat_turn.rs`
(`push_history_scoped`) und die sechs in `remote_files.rs`. Im ganzen
Workspace sind es mindestens 111. Die Heuristik übersieht Dateien, die oben
Testmodule deklarieren. Ein Lint ist im Workspace nicht gesetzt
(`grep -rn unwrap_used Cargo.toml crates/*/Cargo.toml`: kein Treffer).

## 2. Teil 0

Teil 0: entfällt. Alle Aussagen oben sind gelesen oder gezählt. Keine davon
hängt an Laufzeitverhalten, das erst eine laufende App zeigt.

## 3. Ziel und Nicht-Ziele

Das Abnahmekriterium von BL-0231 („ein Test löst den Fehlerfall an Stelle 1
aus und ist gegen den ungefixten Stand rot") wird über T1/T2 erfüllt: Der
`expect` selbst ist nicht auslösbar (§1), der hängende Indikator dagegen
schon, und zwar durch Abbruch des wartenden Futures.

Ziel: Das Warten auf eine Bestätigung räumt Indikator und Registry-Eintrag
auf **jedem** Ausgang ab: Entscheidung, Timeout, geschlossener Kanal, Panic
und Abbruch des Futures. Die Orchestrierung enthält keinen `unwrap`/`expect`
mehr, und ein neuer fällt beim Gate auf.

Nicht-Ziele:
- Kein workspaceweiter Abbau von `unwrap`/`expect` und kein Lint außerhalb
  von `orchestration/` (andere Module: eigenes Item, falls gewünscht).
- Kein Wechsel auf eine andere Mutex-Implementierung, keine neue Abhängigkeit.
- Keine Änderung an `PENDING_ACTION_CONFIRM_TIMEOUT`, an der Bedeutung
  „Timeout = Ablehnung" oder am MCP-Verhalten bei einem Tool-Timeout (die
  UI-Bestätigung läuft danach weiter, Spec 0028 §7).
- Ein Stopp des Chat-Turns bricht ein laufendes Bestätigungswarten
  weiterhin **nicht** ab.
- Keine weiteren Sperrstellen in `session.rs` außer der von
  `pending_action` im Snapshot. `mcp_origin_flags` wird in `chat_turn.rs` und
  `compaction.rs` gesperrt, dazu A4.1. Unter den
  übrigen (`status`, `sessions`, `chat_turn`, `pending_connections`) läuft
  nur Einfügen, Entfernen, Klonen oder Zuweisen, also kein Code, der
  panicken kann.
- `unreachable!` in `action_exec.rs` (für MCP nicht erreichbare
  `GenerateDocument`-Zweige) bleiben. Der Lint betrifft nur `unwrap`/`expect`.

## 4. Anforderungen

**A1 — Bestätigungswarten räumt auf jedem Weg ab**
- A1.1 MUSS: Nach jedem Ausgang des Wartens ist `pending_action` der Sitzung
  wieder `None`, und zwar nach Genehmigung, Ablehnung, Timeout, gedropptem
  Sender, einem Panic im Wartepfad und nach Fallenlassen des wartenden
  Futures.
- A1.2 MUSS: Wird der Future zwischen der Registrierung der Bestätigung und
  ihrer Auflösung fallen gelassen (auch schon während Vorschau oder
  Zweitmeinung), ist der Eintrag für diese `action_id` danach nicht mehr in
  der Registry. Ein späteres Auflösen für sie (`ConfirmationRegistry::resolve`,
  in der App über den Befehl `respond_to_action`) liefert einen Fehler und
  führt nichts aus.
- A1.3 MUSS: Kein Ausgang aus A1.1/A1.2 führt die Aktion aus. Abgeräumt heißt
  nie genehmigt.
- A1.4 MUSS: Der Bestätigungspfad enthält keinen `expect`/`unwrap`. Dass im
  `Confirm`-Zweig ein Empfänger vorliegt, folgt aus dem Code und nicht aus
  einer Laufzeitprüfung.
- A1.5 MUSS: Das bestehende Verhalten bleibt: Timeout = Ablehnung mit
  `RejectionReason::Timeout`, gedroppter Sender = nichts ausführen und
  `earlier_rejection` setzen, MCP-Tool-Timeout lässt die UI-Bestätigung
  weiterlaufen.

**A2 — Vergiftete Sperre von `pending_action`**
- A2.1 MUSS: Ist der Mutex von `pending_action` vergiftet, panicken weder der
  Sitzungs-Snapshot noch das Setzen oder Abräumen im Bestätigungspfad.
  Sie arbeiten mit dem enthaltenen Wert weiter. Der Wert ist ein einfaches
  `Option` und nach jeder Zuweisung in sich stimmig.
- A2.2 MUSS: Das Abräumen aus A1.1/A1.2 panickt nie, auch nicht bei
  vergifteter Sperre von `pending_action` oder der Registry, und auch nicht
  während eines laufenden Panics. Ein Panic in einem Drop während des
  Abwickelns bricht den Prozess ab. Die Registry darf dafür intern tolerant
  werden, ihre öffentliche API bleibt unverändert.

**A3 — SFTP-Dateiaktionen ohne Panic-Pfad**
- A3.1 MUSS: Die Dateiaktionen (`ReadRemoteFile`, `WriteRemoteFile` inkl.
  Backup und Sudo-Rückfall) enthalten keinen `expect`/`unwrap` auf den
  normalen SFTP-Kanal.
- A3.2 MUSS: Wäre der Kanal nach `ensure_sftp_open` trotzdem leer, endet die
  Aktion mit einem Aktionsfehler im Chat mit Fehlercode
  und nie mit einem Panic. Die
  Umwandlung „leerer Kanal → Fehler" liegt an **einer** Stelle, damit T15
  sie an einer frischen Sitzung prüfen kann. Dort ist der Kanal leer,
  solange `ensure_sftp_open` nicht lief.
- A3.3 MUSS: Keine Änderung an Spec 0085 A3. Der normale Kanal wird weiterhin
  nur aus dem Transport der eigenen Sitzung befüllt und lässt sich von
  außerhalb `app-logic` weder ersetzen noch leeren.

**A4 — Übrige Stelle und Absicherung**
- A4.1 MUSS: `mcp_origin_flags` verhält sich bei vergifteter Sperre wie
  A2.1, und zwar an beiden Produktivstellen: beim Schreiben in
  `push_history_scoped` und beim Lesen in der Kompaktierung
  (`compaction.rs`, nach `mem::take` des Verlaufs; ein Panic dort ließe
  den Verlauf leer zurück). Verlauf und Flags bleiben gleich lang.
- A4.2 MUSS: `cargo clippy --workspace --all-targets -- -D warnings` lehnt
  einen neuen `unwrap`/`expect` im Produktivcode von
  `crates/app-logic/src/orchestration/` ab. Testcode (`#[cfg(test)]`) ist
  ausgenommen. `orchestration/` enthält keinen Code hinter `test-support`,
  und `orchestration/test_support.rs` ist reines `#[cfg(test)]`.
- A4.3 SOLL: Wo die Absicherung nur per Kommentar möglich ist, steht die
  Invariante am Typ bzw. Feld, nicht am Aufrufer.

## 5. Design

- Das Abräumen aus A1.1/A1.2 hängt an einem Wert, dessen `Drop` es erledigt,
  und nicht an Anweisungen von Hand vor und nach dem `await`. Er beginnt mit
  der Registrierung, nicht erst beim Setzen von `pending_action`. Er setzt
  `pending_action` nur dann auf `None`, wenn dort noch die eigene
  `action_id` steht. MCP und Chat teilen eine Sitzung, und der Indikator
  einer anderen wartenden Aktion bleibt stehen.
- A2.2 schließt `lock().unwrap()` im `Drop` aus. Den Weg über den vergifteten
  Wert wählt der Coder.
- Keine öffentliche API von `ConfirmationRegistry` ändern (A2.2). `cancel`
  und, für Tests, `contains` existieren bereits.

## 6. Sicherheits-Invarianten

- **Ein Timeout lehnt ab, er gewährt nie** (Confirm): A1.3/A1.5, T8
  (bestehende Timeout-Tests), T3.
- **KI und MCP erreichen nur, was durch Filter und Confirm läuft:** Der
  Umbau berührt nur das Warten, nicht `evaluate_action` und nicht
  `handle_user_decision`. T3 und T10 sichern als Regression, dass ein
  abgeräumter oder schon aufgelöster Eintrag nichts ausführt.
- **Nie hängen:** Der Indikator bleibt nicht mehr stehen (A1.1).
- **Paarung Transport ↔ normaler Kanal** (Spec 0085 A3, Spec 0086 A3):
  A3.3. Die `compile_fail`-Doctests an `lock_sftp` bleiben unverändert grün.
- Redaction, Ledger, Kompaktierung, Rate-Limit-Gate: nicht berührt. Der
  Umbau ändert keinen Inhalt, nur Aufräumen und Fehlerpfade.

## 7. Tests

Alle Tests mit Zeitgrenze, damit ein Hänger rot wird statt zu blockieren.
„Rot heute" heißt: scheitert auf `33d0501`. Der Coder fährt diesen
Gegenbeweis und nennt ihn im Bericht.

- **T1 (A1.1, rot heute).** Aktion mit Confirm vorschlagen, warten bis
  `pending_action == Some(id)`, dann die wartende Task abbrechen.
  Erwartet: `pending_action == None` und der Snapshot meldet
  `has_pending_action == false`. Heute bleibt `Some`.
- **T2 (A1.2, rot heute).** Wie T1. Erwartet zusätzlich: Die Registry enthält
  `id` nicht mehr. Heute bleibt der Eintrag.
- **T3 (A1.3, Regression).** Nach T1 die `id` mit Genehmigung auflösen:
  Das liefert einen Fehler, und es wird kein Kommando ausgeführt. Der
  Transport-Mock zählt `execute` nicht mit, der Test zählt also die
  `chat-action-result`-Events oder ergänzt einen Zähler. Nur Regression:
  Der Empfänger fällt mit dem Future.
- **T4 (A1.2, adversarial, rot heute).** Abbruch **während** der Zweitmeinung
  (Provider-Mock mit hängendem Stream, `pending_action` noch `None`).
  Erwartet: Die Registry enthält `id` nicht mehr, und
  `pending_action == None`. Scheitert, wenn das Abräumen erst beim Setzen
  des Indikators beginnt.
- **T5 (A2.1, adversarial, rot heute).** Den `pending_action`-Mutex im Test
  vergiften (ein Thread panickt mit gehaltener Sperre). Danach laufen
  Snapshot, Confirm-Vorschlag und Genehmigung ohne Panic durch. Die Aktion
  wird genau einmal ausgeführt, und danach ist der Indikator `None`. Heute
  panickt der Snapshot. Scheitert auch, wenn der Guard bei Vergiftung das
  Abräumen auslässt.
- **T6a (A2.2, adversarial).** `pending_action` vergiften. Dann ein Panic
  nach der Registrierung, z. B. ein Emitter-Mock, der beim Vorschlags-Event
  panickt, in einer Task unter `tokio::spawn`. Erwartet:
  `JoinError::is_panic()` und kein Prozessabbruch (ein zweiter Panic im
  Drop während des Abwickelns bräche den Prozess ab). Die Registry
  enthält `id` nicht mehr.
- **T6b (A2.2, adversarial, rot gegen einen Drop mit `lock().unwrap()`).**
  `pending_action` vergiften, warten bis der Indikator `Some(id)` ist (per
  `into_inner` gelesen), dann die Task abbrechen. Erwartet:
  `JoinError::is_cancelled()`, nicht `is_panic()`, und
  `pending_action == None`. tokio 1.53 meldet einen Panic im Drop eines
  abgebrochenen Tasks als `is_panic` (`runtime/task/harness.rs`,
  `cancel_task`, gelesen). Der Test scheitert also an genau diesem Fehler.
- **T6c (A2.2, rot heute).** Unit-Test der Registry: ihre interne Sperre
  vergiften (Thread im Modul panickt mit gehaltener Sperre). Danach
  panicken `register`, `resolve`, `cancel` und `cancel_if_current` nicht und
  verhalten sich wie ohne Vergiftung. Heute panickt `cancel`.
- **T7 (A4.1, rot heute).** `mcp_origin_flags` vergiften, dann eine Nachricht
  pushen und anschließend kompaktieren. Kein Panic, Verlauf und Flags
  bleiben gleich lang, und nach der Kompaktierung ist der Verlauf nicht leer.
- **T8 (A1.5, Regression).** Die bestehenden Tests zu Ablehnung, Timeout
  und Folgerunde (`test_regression_pending_action_cleared_and_turn_completes_after_deny`
  und Nachbarn) bleiben unverändert grün.
- **T9 (A1.5, MCP, Regression).** Der bestehende
  `test_confirm_timeout_returns_timeout_message_without_cancelling_backend_call`
  in `mcp-server` bleibt grün. Er ist die eigentliche Sicherung dafür, dass
  ein Tool-Timeout die UI-Bestätigung nicht abräumt.
- **T10 (A1.3, Regression).** Genehmigung, danach sofort eine zweite
  Auflösung derselben `id`: genau eine Ausführung, die zweite liefert
  einen Fehler, der Indikator ist `None`.
- **T11 (A3.1/A4.2, Nachweis im Bericht).** Der Coder fügt testweise einen
  `unwrap()` in eine Produktivfunktion von `orchestration/` ein und zeigt,
  dass `cargo clippy --workspace --all-targets -- -D warnings` rot wird.
  Dasselbe in einer Testdatei zeigt, dass es grün bleibt. Beide Änderungen
  werden danach verworfen, nicht committet.
- **T12 (A3.3).** Die `compile_fail`-Doctests an `Session::lock_sftp` und
  `SessionTransport::lock` bleiben grün (`cargo test --workspace --doc`).
- **T13 (A3.2, Regression).** Bestehende Tests zu fehlgeschlagenem
  SFTP-Öffnen bei `ReadRemoteFile`/`WriteRemoteFile` bleiben grün. Fehlt
  einer, kommt einer dazu: Transport-Mock mit fehlschlagendem
  `open_sftp`, dann Aktionsfehler mit Code im Chat und kein Panic.
- **T15 (A3.2).** Die eine Stelle, die einen leeren Kanal in einen Fehler
  umwandelt, liefert an einer frischen Sitzung (ohne `ensure_sftp_open`)
  einen `SshError` und keinen Panic. Scheitert, wenn dort weiter
  `expect`/`unwrap` steht oder ein leerer Kanal als Erfolg durchgeht.
- **T14 (A1.1, adversarial).** **Während** des Wartens (vor jeder
  Antwort) gilt `pending_action == Some(id)`, und die Registry enthält `id`.
  Danach genehmigen, und die Aktion läuft. Scheitert gegen einen Guard, der
  sofort fällt (etwa `let _ = …`) und damit vor der Antwort abräumt.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

**K1 — Clippy nimmt Testmodule nicht von selbst aus (zu A4.2).** Die
Formulierung „Testcode (`#[cfg(test)]`) ist ausgenommen" beschreibt das Ziel,
nicht das Verhalten von clippy. Gemessen bei der Umsetzung: Mit
`#![deny(clippy::unwrap_used, clippy::expect_used)]` am Modul `orchestration`
meldet `cargo clippy --workspace --all-targets` **366** Treffer, sämtlich in
den per `#[cfg(test)] mod …;` eingebundenen Testdateien. Die Ausnahme muss
also ausdrücklich gesetzt werden — entweder workspaceweit über
`allow-unwrap-in-tests`/`allow-expect-in-tests` in einer `clippy.toml` oder
lokal als `#[allow(…)]` an den Testmodul-Deklarationen. Umgesetzt wurde die
lokale Variante, damit die Ausnahme denselben Radius hat wie der Lint
(Nicht-Ziel „kein Lint außerhalb von `orchestration/`"); Begründung in
ADR 0082, Abschnitt 5.

**K2 — Ist-Stand bestätigt.** Die Zählung aus §1 stimmt mit dem Code zum
Zeitpunkt der Umsetzung überein: genau 10 `unwrap`/`expect` im Produktivcode
von `orchestration/` (3 in `action_exec.rs`, 6 in `remote_files.rs`, 1 in
`chat_turn.rs`). Kein Widerspruch zum Ist-Stand gefunden.

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `fix(app-logic): clear the pending confirmation on every exit from the wait [BL-0231]` — A1, A2, T1–T6c, T8–T10, T14.
2. `refactor(app-logic): remove panic paths from remote file actions and history flags [BL-0231]` — A3, A4.1, T7, T12, T13, T15.
3. `build(app-logic): deny unwrap and expect in orchestration production code [BL-0231]` — A4.2, A4.3, T11.

Kein Changelog-Fragment (kein nutzersichtbares Verhalten).

**Priorität:** ERHÖHT (Ausführungspfad: Confirm/Timeout, MCP).
Angriffsrichtungen für den Review:
- Aufräumen erst ab dem Indikator statt ab der Registrierung (T4).
- Guard, der sofort fällt und vor der Antwort abräumt (T14).
- Drop, das bei vergifteter Sperre panickt: beim Abbruch als Panic statt
  Abbruch (T6b), während des Abwickelns als Prozessabbruch (T6a).
- Guard, der bei Vergiftung das Abräumen auslässt (T5); Registry, deren
  `cancel` bei Vergiftung panickt (T6c).
- Guard, der den Indikator einer anderen wartenden Aktion derselben
  Sitzung löscht (§5; Review am Code, heute ohne Abbruchpfad nicht auslösbar).
- Abräumen, das an den MCP-Tool-Call gekoppelt wird und so Spec 0028 §7
  bricht (T9, Sicherung liegt im bestehenden `mcp-server`-Test).
- Umbau der SFTP-Stellen, der den Kanal von außen ersetzbar oder leerbar
  macht (T12).

**Aufteilung:** ein Lauf, Opus. Schritt 1 ist Ausführungspfad, Schritte 2
und 3 sind klein und hängen am selben Modul. Ein zweiter Lauf lohnt nicht.

**Berührte Module:** `crates/app-logic/src/orchestration/` (`action_exec.rs`,
`remote_files.rs`, `chat_turn.rs`, Tests), `crates/app-logic/src/session.rs`
(Snapshot), `crates/app-logic/src/confirmation.rs`, `crates/app-logic/src/compaction.rs`,
`crates/app-logic/src/orchestration.rs` (Moduldeklarationen, Lint).

**Melde zurück:** welche Tests auf `33d0501` rot waren (Gegenbeweis), wie
A1.4 ohne Laufzeitprüfung erreicht ist, das Ergebnis von T11 mit beiden
Clippy-Ausgaben, und ob clippy Hilfsfunktionen in per `#[cfg(test)] mod …;`
eingebundenen Testmodulen ohne weitere Konfiguration ausnimmt.
