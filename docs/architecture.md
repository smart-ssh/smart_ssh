# Architektur: `app-logic` und `app-shell`

Spec 0084. Ergänzt die Crate-Übersicht in `AGENTS.md` um die Abgrenzung
zwischen den beiden Crates, die zusammen die Anwendung bilden (Filter-
Engine, Risiko-Klassifizierung, Persistenz-Implementierungen usw. liegen
weiterhin in ihren eigenen Crates, s. `AGENTS.md`).

## Zwei Crates, eine Richtung

```
smart-ssh-community → app-shell → app-logic → ssh-manager-core (crates/core),
                                              ai-providers, ssh-transport,
                                              persistence-sqlite,
                                              credentials-keyring, mcp-server
                      app-shell → direkt auch: ssh-manager-core, ai-providers,
                                  ssh-transport, persistence-sqlite,
                                  credentials-keyring, mcp-server, speech
ai-providers, ssh-transport, persistence-sqlite,
credentials-keyring, mcp-server → ssh-manager-core
speech, real-app-tests, ssh-manager-core → (keine Workspace-Abhängigkeit)
```

Alle Kanten (nur normale Abhängigkeiten zwischen Workspace-Crates):

| Crate (Verzeichnis) | hängt ab von |
|---|---|
| `smart-ssh-community` (`apps/smart-ssh-community`) | `app-shell` |
| `app-shell` | `app-logic`, `ssh-manager-core`, `ai-providers`, `ssh-transport`, `persistence-sqlite`, `credentials-keyring`, `mcp-server`, `speech` |
| `app-logic` | `ssh-manager-core`, `ai-providers`, `ssh-transport`, `persistence-sqlite`, `credentials-keyring`, `mcp-server` |
| `ai-providers`, `ssh-transport`, `persistence-sqlite`, `credentials-keyring`, `mcp-server` | `ssh-manager-core` |
| `ssh-manager-core` (`crates/core`) | — |
| `speech` | — (Spracherkennung, nur von `app-shell` genutzt) |
| `real-app-tests` | — (startet die echte App über tauri-driver, Spec 0090) |

Reproduzierbar mit `cargo metadata --format-version 1 --no-deps` (Pakete
und ihre `dependencies` mit `kind == null`, gefiltert auf Workspace-Namen).

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

Weitere Module, die ohne eigenes `tauri::` auskommen oder nur an `Wiring`/
dem Startablauf hängen, bleiben aus dem jeweils genannten Grund hier:

| Modul | Grund |
|---|---|
| `chat_content.rs`, `db_extensions.rs` | Wenden die Erweiterungen aus `Wiring` an (Spec 0038 Abschnitt 2a), aufgerufen aus dem Start-/Entsperrablauf in `app-shell`. |
| `edit_copies.rs`, `file_clipboard.rs` | Ablage und Aufräumen lokaler Dateien des Dateibrowsers (Spec 0054), vom Command-Layer und vom Start-/Beenden-Ablauf benutzt. |
| `file_ops.rs` | Laufende Datei-Operationen des erhöhten Kanals; hängt an `ElevatedSftpRegistry` (s. o.) und bleibt deshalb bei ihr. |
| `startup_gate.rs` | Torliste vor dem `generate_handler!`-Verteiler (Spec 0101 A16); selbst Tauri-frei, gehört zum Tor, das die Verdrahtung einhängt. |
| `startup_prompt.rs` | Zuordnung Fall → Dialog für die nativen Startdialoge (Spec 0101 A3/A5), arbeitet mit dem nativen Fehlerdialog zusammen. |

Alle übrigen Module in `app-shell` erfüllen die Regel direkt —
`tauri::`/`tauri_plugin_*::`/`AppHandle`/`#[tauri::command]` im eigenen
Code, oder (bei `startup_dialog`) das Paket `rfd`: `android_service`,
`auto_reconnect`, `chat_retention`, `commands`/`commands/*`,
`first_run_notice`, `lib.rs`, `local_file`, `local_server`, `mcp_backend`,
`mcp_settings`, `risk_second_opinion`, `runtime_assumptions` (Test der
Tauri-Async-Runtime-Annahme, Spec 0101 §2 Frage 2), `services` (startet/stoppt
die Hintergrunddienste aus `Wiring` je Entsperrung), `settings_store`,
`single_instance`,
`speech_models`, `ssh_config_apply`/`ssh_config_export`, `startup_dialog`,
`startup_gate_wiring`, `ui_language`, `voice_input`, `window_prompt`,
`wiring`.

## `app-logic`: Module nach Bereich

Wohin neue Logik gehört (alle Tauri-frei; jedes `pub mod` aus `lib.rs`):

**Sitzungen und Orchestrierung**
- `session` — laufende Server-Sessions; `session_extension` — Erweiterungspunkt für Sitzungsdaten.
- `orchestration` — Kernschleife eines Chat-Turns; `compaction` — Kontext-Kompaktierung; `system_prompt` — Texte des System-Prompts.
- `confirmation`, `confirmation_explain` — Bestätigungs-Registry und Erklärung einer Bestätigung.
- `second_opinion` — Abruf-Logik der KI-Zweitmeinung; `rule_suggestions` — Regel-Schnellvorschlag.
- `connect_attempts` — abbrechbare Verbindungsversuche; `test_connection` — Verbindungstest.
- `auto_reconnect` — automatischer Wiederaufbau; `background_keepalive` — Sitzungen im Hintergrund halten.
- `start_directory` — Startverzeichnis einer Sitzung; `folder_upload` — Plan eines rekursiven Ordner-Uploads.
- `mcp_lookup`, `mcp_sessions` — lesende MCP-Tools und MCP-eigene Sitzungen.
- `ai_provider_factory` — baut den konkreten KI-Provider; `ai_usage` — Erweiterungspunkt Nutzungsbeobachtung; `chat_content_extension` — Erweiterungspunkt für Chat-Inhalte.
- `state` — gemeinsamer `AppState`; `events` — Events Richtung Frontend; `dto` — DTOs der IPC-Grenze; `error` — Fehlertyp der Commands.

**Persistenznahe Dienste**
- `servers`, `groups` — Server- bzw. Gruppenverwaltung; `filter_rules` — Filter-Regel-Logik; `policy` — `PolicyStore` ohne Regeln.
- `host_key_store` — persistenter `HostKeyStore`; `database_startup` — Startablauf nach Dateizustand.
- `server_redaction` — Redactor und redigierte Notizen eines Servers.

**Credentials und Secrets**
- `server_credentials` — Formulardaten ↔ Credentials; `ephemeral_credentials` — flüchtiger In-Memory-Store.
- `master_password` — Verpackungsdatei des Master-Passworts; `secret_migration` — einmaliger Umzug der Secrets; `field_content_decryption` — Startschritt zur Feld-Entschlüsselung.
- `mcp_token` — MCP-Token in der verschlüsselten Datenbank; `key_files`, `identity_file` — Schlüsseldateien lesen und übernehmen.
- `startup_choice_dialogs`, `startup_error_messages` — Texte der Startdialoge und Startfehler.

**Backup, Import, Export**
- `backup` — verschlüsseltes Voll-Backup; `ssh_config_import` — Dateizugriff für den `ssh_config`-Import; `document_export` — Dateinamen für KI-Dokumente; `diagnostics` — redigierter Diagnose-Export.

**Plattform-Helfer**
- `data_paths` — Datenpfade; `local_path_grants` — freigegebene lokale Pfade; `owner_names` — Besitzer-/Gruppennamen für den Dateibrowser.
- `single_instance` — Single-Instance-Entscheidung; `logging` — JSON-Lines-Logging; `version` — Version/Build-Hash; `poison` — vergiftungsfeste Sperren (nicht `pub`).
- `test_support` — Testhilfen hinter `test-support` (s. o.).
