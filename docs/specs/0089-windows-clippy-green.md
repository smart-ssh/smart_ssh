# Spec 0089 — Clippy green on Windows

Status: freigegeben · Backlog: BL-0281 · Gate: —
Zweck: Der CI-Job `Test (windows-latest)` kommt wieder über `cargo clippy`
und die Grenzprüfung hinaus, ohne das Verhalten oder die Testabdeckung auf
Unix zu ändern.
Review-Priorität: NORMAL

## 1. Ist-Stand (Stand `4fe796a`)

**CI.** Workflow `Community`, Matrix `ubuntu-latest`, `windows-latest`,
`macos-latest` (`fail-fast: false`); Schritte `cargo fmt --all --check` →
`cargo clippy --workspace --all-targets -- -D warnings` → `cargo test
--workspace` → `cargo build --workspace` → „Grenzprüfung app-logic ist
Tauri-frei" → Node-Setup → `npm ci` → `npm run build`. Unter Windows scheitert jeder Lauf seit `8779ecf` im Schritt
Clippy; `cargo test` lief dort seitdem nie. Letzter Lauf (`4fe796a`):
Ubuntu, macOS, Audit grün, Windows rot mit
`unused variable: metadata` in `crates/ssh-transport/src/local_sftp.rs`
(`owner_ids`).

**Gemessen:** Cross-Clippy im Container `rust:trixie` mit Target
`x86_64-pc-windows-gnu` und mingw, Befehl
`cargo clippy --locked --keep-going --target x86_64-pc-windows-gnu --workspace --all-targets -- -D warnings`,
in vier Runden; zwischen den Runden wurden die gefundenen Stellen in einer
Wegwerf-Kopie behelfsmäßig überbrückt, um die dahinterliegenden sichtbar zu
machen. Ergebnis: genau **vier Stellen**, danach Exit 0.

| Nr. | Ort | Fehler | Ziel |
|---|---|---|---|
| W1 | `ssh-transport/src/local_sftp.rs`, `owner_ids` | `metadata` unbenutzt im Nicht-Unix-Zweig | Bibliothek |
| W2 | `app-shell/src/commands/sftp.rs`, Modul `sftp_mutation_tests` | `std::os::unix` (Import, `symlink`) und `Permissions::from_mode` — 4 Fehler E0433/E0599 | Testcode |
| W3 | `app-shell/src/ssh_config_export/tests.rs`, `t_6_3_14_symlink_auf_ssh_verzeichnis_wird_erkannt` | `link` unbenutzt, weil nur im `#[cfg(unix)]`-Block verwendet | Testcode |
| W4 | `ssh-transport/tests/fixtures/test_server.rs`, `setstat` | `PermissionsExt::from_mode` ohne `cfg(unix)`; nach dem Eingrenzen `path`/`attrs` unbenutzt | Testcode (Integrationstest) |

**W5 — Grenzprüfung (gelesen, nicht ausgeführt).** Der Schritt ist in
Bash geschrieben (`tree_output="$(…)"`, `if [ $? -ne 0 ]`), der Workflow
setzt weder `shell:` noch `defaults:`. Auf Windows-Runnern ist die
Standard-Shell PowerShell Core (`pwsh`; GitHub-Dokumentation,
<https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax>,
Abschnitt `jobs.<job_id>.steps[*].shell`); dort ist das Skript ungültig. Windows hat den Schritt nie erreicht.

Alle übrigen Crates, auch `app-logic`, `app-shell` (Bibliothek) und
`apps/smart-ssh-community`, sind unter dem Windows-Target Clippy-sauber.

**Grenzen der Messung:**
- Target `-gnu` statt `-msvc` wie in der CI. Code, der nach
  `target_env` unterscheidet, wäre nicht erfasst;
  `grep -rn target_env crates apps --include='*.rs'` liefert keinen Treffer
  (dieselbe Suche nach `target_os`: 31).
- `cargo test`, `cargo build`, `npm ci` und `npm run build` unter Windows
  sind nicht gemessen (hier nicht ausführbar; lief dort seit `8779ecf`
  nie). Ob sie scheitern, zeigt erst der CI-Lauf nach dem Push (§8, R1).

## 2. Teil 0

Teil 0: entfällt — die Clippy-Fehler sind vollständig gemessen, W5 ist
gelesen und dokumentiert. Was danach unter Windows scheitert, lässt sich
vor dem Push nicht klären und ändert den Zuschnitt dieser Spec nicht (R1).

## 3. Ziel und Nicht-Ziele

Ziel: A1–A5.

Nicht-Ziele:
- Keine Änderung am Workflow außer A5 (keine neuen oder entfernten
  Schritte, keine Änderung an Matrix, Flags oder Befehlen).
- Keine Unix-Tests auf Windows portieren; kein neuer Windows-Test.
- Kein Beheben von Fehlern, die erst der nächste CI-Lauf zeigt (R1).

## 4. Anforderungen

- **A1 MUSS:** `cargo clippy --workspace --all-targets -- -D warnings` ist
  für ein Windows-Target fehlerfrei (W1–W4).
- **A2 MUSS:** Auf Unix bleibt alles gleich: `owner_ids` liefert weiterhin
  `uid`/`gid`; jeder Test, der heute auf Linux/macOS läuft, läuft dort
  weiterhin (gleiche Anzahl in `cargo test --workspace`); der Test-Server
  der Integrationstests wendet `setstat`-Rechte auf Unix weiterhin an.
- **A3 MUSS:** Keine Abschwächung der Lint-Schranke: kein `#[allow(...)]`
  auf Crate- oder Modulebene, kein Entfernen von `-D warnings`. Eine
  Unterdrückung ist nur an der einzelnen Stelle und nur für den
  Nicht-Unix-Zweig zulässig.
- **A4 SOLL:** Tests, die unter Windows nicht übersetzt werden, sind als
  „nur Unix" erkennbar (Bedingung am Test bzw. Modul, nicht stilles
  Entfernen).
- **A5 MUSS:** Der Schritt „Grenzprüfung app-logic ist Tauri-frei" läuft
  auf allen drei Betriebssystemen in Bash (`shell: bash` an diesem
  Schritt); Skript und Regex bleiben unverändert. Dadurch kommt auf allen
  Runnern `pipefail` hinzu; für dieses Skript ist das wirkungsneutral.

## 5. Design

Nichts vorzugeben; wie ein Parameter im Nicht-Unix-Zweig verbraucht bzw.
ein Test eingegrenzt wird, wählt die Umsetzung.

## 6. Sicherheits-Invarianten

W2 enthält die Symlink-Schutztests für rekursives Löschen und chmod
(`delete_recursive`/`chmod_recursive` folgen keinem Symlink an der
Wurzel). Sie bleiben auf Unix unverändert aktiv (A2); unter Windows gab es
sie nie (der Code übersetzte dort nicht). Keine Sicherheitslogik wird
geändert.

## 7. Tests

Kein neuer Test. Nachweise:
- **N1 (A1):** Cross-Clippy wie in §1 mit Exit 0 auf dem Endstand. Der
  Coder kann das nicht selbst ausführen (kein Windows-Target installiert);
  er prüft jede Stelle am Code, der Nachweis erfolgt vor dem Push.
- **N2 (A2):** `cargo test --workspace` auf macOS: Summe `passed` und
  Summe `ignored` vor und nach der Änderung gleich (im Bericht alle vier
  Zahlen). Dazu Sichtprüfung im Review: der Unix-Zweig von `owner_ids`
  (W1) und die Assertion von W3 stehen im Diff unverändert und nicht unter
  einer Nicht-Unix-Bedingung.
- **N3 (A3):** Jedes neue `allow(` im Diff steht in
  `cfg_attr(not(unix), allow(...))` an einem einzelnen Item oder Statement;
  kein `#![allow]`, kein `allow` an einer `mod`-Deklaration.
- **N4 (A5):** Die einzige Änderung unter `.github/` ist `shell: bash` am
  Schritt der Grenzprüfung.

## 8. Offene Punkte

Keine.

**R1 (bewusst offen):** Seit `8779ecf` liefen unter Windows alle Schritte
nach Clippy nie. Scheitert dort nach dem Push noch etwas, zeigt das erst
der CI-Lauf. BL-0281 gilt erst als erledigt, wenn der Job auf allen drei
Betriebssystemen `success` meldet; bis dahin folgen Ergänzungen dieser
Spec mit eigenem Lauf.

## 9. Klarstellungen

(leer)

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `fix(ssh-transport): consume the metadata argument on non-unix targets [BL-0281]` — W1.
2. `test(ssh-transport): limit the permission handling of the test server to unix [BL-0281]` — W4.
3. `test(app-shell): mark the unix-only file tests as such [BL-0281]` — W2, W3.
4. `fix(ci): run the app-logic boundary check in bash on every runner [BL-0281]` — W5.

**Priorität:** NORMAL.

**Aufteilung:** ein Lauf, Sonnet. Vier kleine, zusammengehörige Commits.

**Berührte Module:** `crates/ssh-transport` (`local_sftp.rs`,
`tests/fixtures/test_server.rs`), `crates/app-shell`
(`commands/sftp.rs`, `ssh_config_export/tests.rs`),
`.github/workflows/community.yml`.

**Melde zurück:** `passed`/`ignored` vor/nach (N2), je Stelle die gewählte Form,
ob `cargo test` weitere `cfg(unix)`-Abhängigkeiten zeigt, die Clippy nicht
sieht (z. B. Tests, die übersetzen, aber Unix-Pfade voraussetzen).
