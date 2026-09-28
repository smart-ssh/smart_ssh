# Architektur: `app-logic` und `app-shell`

Spec 0084. Ergänzt die Crate-Übersicht in `CLAUDE.md` um die Abgrenzung
zwischen den beiden Crates, die zusammen die Anwendung bilden (Filter-
Engine, Risiko-Klassifizierung, Persistenz-Implementierungen usw. liegen
weiterhin in ihren eigenen Crates, s. `CLAUDE.md`).

## Zwei Crates, eine Richtung

```
app-shell  →  app-logic  →  core, ai-providers, ssh-transport,
                             persistence-sqlite, credentials-keyring,
                             mcp-server
```

`app-shell` hängt von `app-logic` ab, nie umgekehrt. `app-logic` kennt
`app-shell` nicht — sie ist die Tauri-freie Anwendungslogik: Chat-Turn-
Orchestrierung, Kompaktierung, Bestätigungs-Registry, Verbindungstest,
Server-/Gruppen-/Credential-Verwaltung, DTOs, gemeinsamer App-Zustand
(`AppState`), Logging-Aufbau, Versions-/Build-Hash-Anzeige.

`app-shell` bleibt, was Spec 0038 schon verlangte — ein dünner Tauri-
Wrapper: Commands/Events, Fenster-/Plugin-Aufbau, native Dialoge. Neu ist
nur, dass diese Abgrenzung jetzt eine Crate-Grenze ist, nicht mehr nur ein
Namensraum innerhalb derselben Crate.

## Die Regel: was gehört wohin

**Ein Modul gehört nach `app-shell`, wenn es Tauri direkt braucht** —
`tauri::`, `tauri_plugin_*::`, `AppHandle`, `#[tauri::command]`, oder das
Paket `rfd` (das native Dialoge zeigt, bevor `tauri::Builder::default()`
überhaupt läuft, s. `app-shell/src/startup_dialog.rs`s Moduldoc). Alles
andere gehört nach `app-logic`.

**Eine Ausnahme, bewusst in die andere Richtung: der erhöhte SFTP-Kanal**
(Spec 0067 A — der `sudo`-Dateibrowser-Modus). `ElevatedSftpRegistry`,
`ElevatedSftp`, `ElevationContext` (`app-shell/src/elevated_sftp.rs`) und
der Zugangsnachweis `BrowserAccess` (`app-shell/src/commands/
elevation.rs`, privates Feld, nur dort konstruierbar) sind selbst
Tauri-frei — sie bleiben trotzdem in `app-shell`. Grund: Spec 0067 A
verlangt, dass die KI-Orchestrierung und die MCP-Anbindung den erhöhten
Kanal nie erreichen können, "kein Typ, keine Funktion, kein Feld" (Spec
0084, A1). Läge die Registry in `app-logic`, könnte `app-logic::
orchestration` (die KI-Logik) sie direkt importieren — die Compiler-
Garantie wäre weg, es bliebe nur noch eine Konvention. Mit der Registry in
`app-shell` gibt es für `app-logic` schlicht keinen Pfad dorthin: ein
Verstoß wäre ein Kompilierfehler (`unresolved import`), keine stille
Möglichkeit.

Dieselbe Ausnahme gilt für die zugehörigen Testhilfen: `app-shell::
test_support::elevation` (Fixtures, `interleave_hook`) und die beiden
Regressionstests `test_ai_and_mcp_file_actions_never_use_the_elevated_
channel`/`test_mcp_write_actions_never_use_the_elevated_channel`
(`app-shell::commands::elevation::browser_channel_tests`) — sie müssten
sonst denselben verbotenen Pfad nach `elevated_sftp`/`BrowserAccess`
nehmen. `docs/adr/0077-app-logic-crate-split-decisions.md` hält die
Einzelentscheidungen dieser Aufteilung fest.

## Das Feature `test-support`

Manche Testhilfen braucht keine der beiden Crates allein, sondern beide:
`app-logic`s eigene Tests **und** `app-shell`s Tests, die über die
Crate-Grenze hinweg dieselben In-Memory-Doubles/Fixtures aufbauen wollen
(`InMemoryProfileStore`, `InMemoryCredentialStore`,
`session_with_transport`, `policy::NoRulesPolicyStore`, u. a.).

Diese Helfer liegen in `app_logic::test_support`/`app_logic::policy`
hinter `#[cfg(any(test, feature = "test-support"))]` statt reinem
`#[cfg(test)]` — ein `#[cfg(test)]` allein wäre außerhalb der eigenen
Crate unsichtbar, ein Konsument (`app-shell`) sähe die Typen gar nicht.
`app-shell/Cargo.toml` aktiviert das Feature ausschließlich unter
`[dev-dependencies]`:

```toml
[dev-dependencies]
app-logic = { path = "../app-logic", features = ["test-support"] }
```

Dasselbe Muster wie die bereits bestehende Dev-Abhängigkeit auf
`ssh-manager-core`s `test-support`-Feature (`app-shell/Cargo.toml`, Spec
0020 Abschnitt 6). Kein Produktivbau aktiviert das Feature — `cargo tree
-p smart-ssh-community -e features` zeigt es nicht (A5, T4).

**Was bewusst NICHT hinter diesem Feature liegt**: die Testhilfen für den
erhöhten Kanal (voriger Abschnitt) — sie bleiben in `app-shell` und sind
von `app-logic` aus unter keinem Feature erreichbar.

## Der CI-Schritt: die Grenze am Compiler, nicht nur am Auge

`.github/workflows/community.yml` prüft nach jedem `cargo build`, dass
`app-logic`s gesamter Abhängigkeitsbaum — normale, Build- und
Dev-Abhängigkeiten, alle Features — kein Paket `tauri`, `tauri-*`, `wry`,
`tao` oder `rfd` enthält:

```bash
cargo tree -p app-logic -e normal,build,dev --all-features --prefix none \
  | grep -E '^(tauri|tauri-[a-zA-Z0-9_-]*|wry|tao|rfd)( |$)'
```

Ein Treffer lässt den Schritt fehlschlagen. `-p app-logic`, nicht an der
Workspace-Wurzel — `app-shell`s eigener Baum enthält `tauri` weiterhin,
völlig zu Recht. `--prefix none` liefert eine Paketzeile ohne
Baum-Zeichnung, damit der Regex-Anker `^` exakt den Paketnamen trifft statt
einer beliebigen Teilzeichenkette.

## Ausnahmen von der Regel (A4)

Jedes Modul, das nach `app-shell` gehört, aber die Regel oben ("Tauri
direkt gebraucht") nicht offensichtlich erfüllt, steht hier mit Grund:

| Modul | Grund |
|---|---|
| `elevated_sftp.rs`, `test_support::elevation`, `commands::elevation::browser_channel_tests`s T5/T5b | Spec 0067 A / Spec 0084 A1 — erhöhter Kanal, s. oben. Selbst Tauri-frei. |
| `event_emitter.rs` | Trägt `impl EventEmitter for TauriEventEmitter` — ein Newtype um `tauri::AppHandle`, nötig weil die Orphan-Rule `impl EventEmitter for tauri::AppHandle` in `app-logic` verbietet (weder Trait noch Typ sind dort definiert). |
| `test_support.rs`s `MockAiProvider` (die undifferenzierte Variante) | Nur von `commands::ai_providers`s eigenem Test gebraucht, kein Cross-Crate-Bedarf — bleibt deshalb lokal statt hinter `test-support`. |

Alle übrigen Module in `app-shell` (`chat_retention`, `commands`/
`commands/*`, `first_run_notice`, `local_server`, `mcp_backend`,
`mcp_settings`, `risk_second_opinion`, `ssh_config_apply`/
`ssh_config_export`, `startup_dialog`, `wiring`, `lib.rs`) erfüllen die
Regel direkt: `tauri::`/`tauri_plugin_*::`/`AppHandle`/
`#[tauri::command]` im eigenen Code, oder (`startup_dialog.rs`) das Paket
`rfd`.
