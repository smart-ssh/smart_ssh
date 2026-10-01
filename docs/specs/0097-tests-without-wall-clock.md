# Spec 0097 — Tests ohne Wanduhr

Status: freigegeben · Backlog: BL-0219, BL-0253, BL-0278 · Gate: release-1.0/K
Zweck: Tests, die unter Last scheitern, weil sie auf echte Zeit statt auf ein Ereignis warten, deterministisch machen und das lokale Gate um den Build ohne Test-Features ergänzen.
Review-Priorität: NORMAL

## 1. Ist-Stand (Stand `191d839`)

**Beobachtete Wackler.** Der Frontend-Test „a drag-and-drop upload after
switching uses the elevated channel (no stale closure)“ in
`FileBrowserPanel.test.tsx` scheiterte zweimal an einem Tag: einmal lokal
parallel zu einem Rust-Build, einmal in der Community-CI (Ubuntu). Beide
Wiederholungen waren grün. Ein rotes Gate ist dadurch mehrdeutig.

**Lokal nicht erzwingbar.** Gemessen unter CPU-Vollast (so viele Busy-Loops
wie Kerne): `cargo test --workspace` 3× grün (4689 Tests), `npx vitest run`
8× grün, dazu 4 gleichzeitige `vitest`-Läufe grün. Ein „N-mal grün“ belegt
hier also nichts. Die Tests in §7 arbeiten deshalb mit einer
**Verzögerungsprobe**: eine künstliche Verzögerung an der Stelle, auf die der
Test wartet, macht den Zeitbezug deterministisch sichtbar.

**Fälle mit echtem Zeitbezug** (gelesen, nicht unter Last reproduziert):

| # | Test | Worauf er wartet | Warum er unter Last scheitern kann |
|---|---|---|---|
| F1 | `FileBrowserPanel.test.tsx`, „a file opened elevated is uploaded elevated even after the toggle is off …“ | `findByText(…, { timeout: 4000 })` auf die Meldung „wurde lokal geändert“ | Die Meldung hängt an einem echten 2-s-Intervall (`POLL_INTERVAL_MS`, `useLocalEditSession`), die Frist ist fest 4 s. Liegt die Ereignisschleife mehr als ~2 s zurück, kommt der Tick zu spät. |
| F2 | `FileBrowserPanel.test.tsx`, „… (no stale closure)“ | `waitFor` mit Standardfrist 1000 ms auf den Upload-Spy | Kein Timer im Produktcode. Reines Ereigniswarten mit knapper Frist. |
| F3 | `tests_core.rs`, `test_execute_suggested_command_cancellation_returns_partial_output` | `sleep(50 ms)`, danach `resolve(...).expect(...)` | **Kein Wackler.** Die Registrierung (`running_command_cancellations.register` in `action_exec.rs`) läuft synchron vor dem ersten `.await` von `exec_future`, beide Futures stehen im selben `tokio::join!`. Der Fall bleibt unverändert. |
| F4 | `tests_core.rs`, `test_slow_session_does_not_block_concurrent_session_via_shared_manager` | `timeout(100 ms)` um den Turn der schnellen Sitzung | 100 ms echte Zeit für einen vollständigen Turn. |
| F5 | `ssh-transport/tests/integration.rs`, `test_execute_cancellable_returns_partial_output_on_cancel` | Abbruch nach `sleep(200 ms)`, dann `stdout == "first line\n"` | Ist die erste Zeile nach 200 ms noch nicht da, ist `stdout` leer. |
| F6 | `mcp-server/tests/integration.rs`, `test_confirm_timeout_over_real_http` | Server-Timeout 50 ms, Backend-Verzögerung 300 ms, `elapsed < 300 ms` | 250 ms Spielraum für einen HTTP-Rundlauf. |

Zu F5: `execute_cancellable` wählt per `tokio::select!` ohne `biased` zwischen Abbruch und Kanal (`transport.rs`). Liegen Daten und Abbruch zugleich an, entscheidet der Zufall. Ein reines Ereignis „erste Zeile ist beim Client“ gibt es ohne Eingriff in den Produktcode nicht (gelesen, nicht ausgeführt).

**Frontend-Voreinstellung:** Es gibt keine globale `asyncUtilTimeout`- oder
`testTimeout`-Einstellung (`vite.config.ts`, `src/test-setup.ts`). Es gelten
die Voreinstellungen, nachgeschlagen in `node_modules`: `asyncUtilTimeout`
1000 ms (`@testing-library/dom`, `config.js`) und `testTimeout` 5000 ms
(vitest 3.2.7, `resolved.testTimeout ??= … 5e3`).

**Schutztests gegen katastrophales Rückverfolgen** (Nicht-Ziel, §3):
gemessen `t_musterabgleich_bleibt_bei_bosartigem_muster_schnell` 0,00 s bei
Schranke 1 s, `test_secret_check_stays_fast_on_adversarial_long_input`
0,60 s bei Schranke 10 s.

**Gate (BL-0278):** Der Abschnitt „Before every commit“ der `CLAUDE.md` im
Wurzelverzeichnis nennt im Rust-Block `cargo fmt`, `cargo clippy --all-targets` und
`cargo test --workspace`. Beide Clippy- und Test-Läufe bauen mit dem Feature
`test-support`. Ein Aufruf eines Test-Zugangs aus Produktivcode fällt nur bei
`cargo build --workspace` auf. Die CI fährt ihn (`community.yml`), lokal fehlt er.

## 2. Teil 0

Teil 0: entfällt. Die Fälle F1–F6 sind am Code gelesen. Ob ein Fall sich
anders verhält als beschrieben, zeigt die Verzögerungsprobe in §7 sofort, und
das ändert nur die Behebung des Falls, nicht den Zuschnitt.

## 3. Ziel und Nicht-Ziele

Ziel: Kein Test der Fälle F1, F2, F4–F6 hängt davon ab, wie schnell die
Maschine ist. Neue Fälle derselben Art, die beim Durchsehen auffallen, werden
gleich behandelt (A4).

Abgrenzung zu den Backlog-Items:
- Der zweite in BL-0219 genannte Fall, `test_sftp_upload_download_roundtrip`,
  ist durch Spec 0093 A7 erledigt: Der Testserver antwortet auf `write` und
  `close` erst nach `flush`. Er steht deshalb nicht in dieser Spec.
- Die Akzeptanz „20 Gate-Läufe ohne Fehlschlag, davon 5 unter Last“
  (BL-0219) bzw. „20× unter Last“ (BL-0253) wird **ersetzt** durch die
  Verzögerungsproben T1, T2, T4–T6 und die Rauchprobe in §7. Grund: Unter Last ließ
  sich lokal kein Fehlschlag erzwingen (§1), 20 grüne Läufe belegen also
  nichts.

Nicht-Ziele:
- Kein Produktcode ändert sein Verhalten. Erlaubt ist nur, eine Zeitkonstante
  für Tests einstellbar zu machen, falls ein Test sie sonst nicht steuern kann.
- Die Schutztests gegen katastrophales Rückverfolgen bleiben (großer Abstand
  zur Schranke, gemessen).
- Kein `#[ignore]`, kein `.skip`, kein Wiederholungs-Plugin (`retry`).
- Keine pauschale Anhebung von Fristen ohne Zuordnung zu einem Fall.
  Ausgenommen sind die Fristen aus A4.

## 4. Anforderungen

- **A1 MUSS:** Ein Test, der auf einen Timer des Produktcodes wartet (F1),
  steuert die Zeit selbst. Sein Ergebnis hängt nicht von der Dauer eines
  echten Intervalls ab.
- **A2 MUSS:** Ein Test, der auf ein Ereignis wartet (F2), wartet auf
  das Ereignis selbst, nicht auf eine geschätzte Dauer. Eine Obergrenze gegen
  Hängen ist erlaubt und muss großzügig sein. Der gute Fall endet sofort, die
  Grenze greift nur, wenn etwas kaputt ist. Richtwert: 5 s wie in den übrigen
  Rust-Tests.
- **A3 MUSS:** Ein Test, der prüft, dass etwas **nicht** blockiert oder
  **früher** endet (F4, F6), prüft das über Reihenfolge oder ein Signal statt
  über ein enges Zeitfenster. Ist ein Zeitvergleich unvermeidbar, liegt
  zwischen gutem und schlechtem Fall mindestens Faktor 20.
  Für F5 gilt dasselbe: Der Test wartet auf ein Signal des Testservers,
  dass die erste Zeile gesendet ist. Danach wartet er fest 1 s, erst dann
  bricht er ab (§1, Zu F5). Die Zustellung über localhost liegt
  erwartungsgemäß weit unter 1 s (nicht gemessen; T5 und die Rauchprobe
  zeigen es), und die Obergrenze von 5 s bleibt darüber. Der Produktcode bleibt
  unverändert.
- **A4 MUSS:** Die Frontend-Tests bekommen eine gemeinsame Obergrenze für
  asynchrones Warten, die A2 genügt, statt einzelner Fristen je Aufruf.
  Die Frist je Test (`testTimeout`) liegt deutlich darüber, damit ein
  Wackler als fehlgeschlagene Erwartung mit Inhalt erscheint und nicht als
  „Test timed out“.
  Stellen mit eigener Frist werden durchgesehen: Wartet eine davon auf einen
  echten Timer, gilt A1. Dasselbe Durchsehen gilt für die Rust-Tests mit
  echter Zeit (`tokio::time::sleep` vor einer Erwartung, `timeout` unter
  1 s). Jeder weitere Fall kommt mit Einordnung in den Bericht und wird nach
  A1–A3 behoben oder begründet belassen.
- **A5 MUSS:** Jeder behobene Test prüft weiterhin dasselbe Verhalten. Die
  Verzögerungsprobe (T1, T2, T4–T6) belegt, dass er bei kaputter Implementierung
  weiter scheitert.
- **A6 MUSS:** Der Rust-Block im Abschnitt „Before every commit“ der
  Wurzel-`CLAUDE.md` enthält `cargo build --workspace` nach
  `cargo test --workspace`, mit einem Satz, was
  dieser Schritt zusätzlich fängt.

## 5. Design

Keine Vorgaben über §4 hinaus. Welche Mittel der Coder wählt (Fake-Timer,
Barriere, Kanal, Notify, ein Ereignis aus dem Mock), entscheidet er am Code.

## 6. Sicherheits-Invarianten

Keine berührt: Es ändern sich nur Tests, ihre Hilfen und die Doku des Gates.
Falls A1 eine Zeitkonstante einstellbar macht, bleibt ihr Produktivwert
unverändert, und nur Testcode setzt einen anderen.

## 7. Tests

Für jeden Fall eine **Verzögerungsprobe**. Die Probe wird gefahren und im
Bericht belegt, aber **nicht committet**:

- **T1 (F1):** `POLL_INTERVAL_MS` probeweise auf 10 000 ms setzen. Der alte
  Test scheitert, der neue bleibt grün. Gegenprobe: Den Aufruf, der die
  Änderung meldet, probeweise entfernen, dann muss der neue Test rot werden.
- **T2 (F2):** Den `sftpExists`-Mock, der vor dem Upload aufgerufen wird,
  probeweise 1500 ms verzögern. Den Upload-Mock selbst zu verzögern genügt
  nicht, weil der Spy den Aufruf schon beim Aufrufen aufzeichnet. Der alte Test
  scheitert, der neue bleibt grün. Gegenprobe: Den Upload über den erhöhten
  Kanal probeweise auf den normalen umbiegen, dann muss der neue Test rot
  werden.
- **T3 (F3):** entfällt, F3 ist kein Wackler (§1).
- **T4 (F4):** Den Turn der schnellen Sitzung probeweise um 300 ms verzögern
  (ohne Sperre). Der alte Test scheitert, der neue bleibt grün. Gegenprobe:
  Die langsame Sitzung eine Sperre halten lassen, die die schnelle braucht,
  dann muss der neue Test rot werden.
- **T5 (F5):** Die erste Ausgabezeile des Testservers probeweise um 500 ms
  verzögern. Der alte Test scheitert, der neue bleibt grün, weil er auf das
  Signal „gesendet“ wartet. Gegenprobe: Den
  Abbruch wirkungslos machen, dann muss der neue Test an der Obergrenze
  scheitern.
- **T6 (F6):** Den HTTP-Weg probeweise um 300 ms verzögern. Der alte Test
  scheitert, der neue bleibt grün. Gegenprobe: Das Server-Timeout
  wirkungslos machen, dann muss der neue Test rot werden.
- **T7 (A4):** Ein Frontend-Test, der auf ein Ereignis nach 1500 ms wartet
  und keine eigene Frist setzt, besteht mit der gemeinsamen Obergrenze und
  scheitert ohne sie. Nur als Probe, nicht committen.
- **T8 (A6):** Ein probeweise eingefügter Aufruf eines Test-Zugangs aus
  Produktivcode (etwa `parts_mut_for_tests`) lässt `cargo build --workspace`
  scheitern, während `cargo test --workspace` grün bleibt. Nicht committen.
- **Rauchprobe:** Das volle Gate zweimal nacheinander grün, ohne zusätzliche
  Last. Gezielte Lastläufe gibt es nicht: Die Verzögerungsproben belegen den
  Zeitbezug, und die Maschine ist ein Arbeitsrechner.

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

- **K1 (F6):** Es genügt nicht, nach der Antwort zu prüfen, dass das Backend noch
  nicht fertig ist, solange das Backend nur eine feste echte Zeit wartet: Unter
  Last kann der Rundlauf länger dauern, dann ist es doch fertig. Das Backend
  wartet deshalb auf eine Freigabe durch den Test, die erst nach dem Eintreffen
  der Antwort kommt, wie bei F4.

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `test(frontend): wait on events and controlled timers instead of wall-clock time [BL-0253]` — F1, F2, A4 (Frontend-Teil).
2. `test(app-logic): prove session independence without a tight time window [BL-0219]` — F4.
3. `test(ssh-transport): cancel only after the test server has sent the first line [BL-0219]` — F5.
4. `test(mcp-server): prove the confirmation timeout without a tight time window [BL-0219]` — F6.
5. `docs(claude): add the build without test features to the local gate [BL-0278]` — A6.

Weitere Fälle aus A4 kommen in den Commit ihrer Schicht (1–4). Fälle in
anderen Crates bekommen einen eigenen Commit `test(<crate>): …` mit
derselben Form.

**Priorität:** NORMAL.

**Aufteilung:** ein Lauf auf Sonnet. Es ändern sich nur Tests und Doku, und
die Fälle sind unabhängig voneinander, aber klein.

**Berührte Module:** Frontend-Tests und Test-Setup,
`crates/app-logic` (Tests von `action_exec`), `crates/ssh-transport/tests`,
`crates/mcp-server/tests`, `CLAUDE.md`.

**Melde zurück:** je Fall die Verzögerungsprobe und die Gegenprobe mit
Ergebnis (alt rot / neu grün / Gegenprobe rot), die Liste der in A4
durchgesehenen Stellen mit Einordnung, die zwei Gate-Läufe der Rauchprobe.
