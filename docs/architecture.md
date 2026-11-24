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
überhaupt läuft, s. `crates/app-shell/src/startup_dialog.rs`s Moduldoc). Alles
andere gehört nach `app-logic`.

**Eine Ausnahme, bewusst in die andere Richtung: der erhöhte SFTP-Kanal**
(Spec 0067 A — der `sudo`-Dateibrowser-Modus). `ElevatedSftpRegistry`,
`ElevatedSftp`, `ElevationContext` (`crates/app-shell/src/elevated_sftp.rs`) und
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

Sitzungen schließt ausschließlich `app_shell::sessions::close_session`
(Issue #816, Spec 0084 A2.1): sie widerruft den erhöhten Kanal, entfernt
die Pfadfreigaben, lehnt die wartende Bestätigung ab, bricht einen
laufenden KI-Befehl ab, trennt und meldet den Status. `disconnect`, der
Backup-Import (`AppOpenSessions`) und einbettende App-Crates bzw.
Erweiterungen nutzen nur diesen Weg; die Registry selbst bleibt
`pub(crate)`. Ein Wächtertest verbietet andere Aufrufe von
`SessionManager::remove`.

Dieselbe Ausnahme gilt für die zugehörigen Testhilfen: `app-shell::
test_support::elevation` (Fixtures, `interleave_hook`) und die beiden
Regressionstests `test_ai_and_mcp_file_actions_never_use_the_elevated_
channel`/`test_mcp_write_actions_never_use_the_elevated_channel`
(`app-shell::commands::elevation::browser_channel_tests`) — sie müssten
sonst denselben verbotenen Pfad nach `elevated_sftp`/`BrowserAccess`
nehmen. `docs/adr/0077-app-logic-crate-split-decisions.md` hält die
Einzelentscheidungen dieser Aufteilung fest.

Die reine SFTP-Logik des Dateibrowsers (rekursives Löschen, chmod/chown,
Modus-Bits, Eigentümer-Namen, Überschreiben-Umbenennen, Baum-Durchlauf)
liegt Tauri-frei in `app-logic::sftp_ops` (Issue #849). Sie arbeitet auf
`&mut dyn SftpSession` und dem kleinen Trait `OpProgress`; `app-shell` löst
den `BrowserChannel` zu einer `SftpSession` auf, ruft hinein und implementiert
`OpProgress` für `OpHandle`. `app-logic` sieht keinen Typ des erhöhten Kanals;
ein Quelltext-Scan (`browser_channel_tests`) sichert das zusätzlich zum Compiler.

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
`crates/app-shell/Cargo.toml` aktiviert das Feature ausschließlich unter
`[dev-dependencies]`:

```toml
[dev-dependencies]
app-logic = { path = "../app-logic", features = ["test-support"] }
```

Dasselbe Muster wie die bereits bestehende Dev-Abhängigkeit auf
`ssh-manager-core`s `test-support`-Feature (`crates/app-shell/Cargo.toml`, Spec
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
| `crates/app-shell/src/elevated_sftp.rs`, `test_support::elevation`, `commands::elevation::browser_channel_tests`s T5/T5b | Spec 0067 A / Spec 0084 A1 — erhöhter Kanal, s. oben. Selbst Tauri-frei. |
| `crates/app-shell/src/event_emitter.rs` | Trägt `impl EventEmitter for TauriEventEmitter` — ein Newtype um `tauri::AppHandle`, nötig weil die Orphan-Rule `impl EventEmitter for tauri::AppHandle` in `app-logic` verbietet (weder Trait noch Typ sind dort definiert). |
| `test_support.rs`s `MockAiProvider` (die undifferenzierte Variante) | Nur von `commands::ai_providers`s eigenem Test gebraucht, kein Cross-Crate-Bedarf — bleibt deshalb lokal statt hinter `test-support`. |

Extension crates use `app_logic::ext` (Spec 0038, "Erweiterungs-API") as the
single supported surface of `app-logic`, next to the root re-exports of
`app-shell` (`Wiring`, `DbExtension`, `BackgroundService`, `ServiceContext`,
`RunningService`).

Weitere Module, die ohne eigenes `tauri::` auskommen oder nur an `Wiring`/
dem Startablauf hängen, bleiben aus dem jeweils genannten Grund hier:

| Modul | Grund |
|---|---|
| `crates/app-shell/src/chat_content.rs`, `crates/app-shell/src/db_extensions.rs` | Wenden die Erweiterungen aus `Wiring` an (Spec 0038 Abschnitt 2a), aufgerufen aus dem Start-/Entsperrablauf in `app-shell`. |
| `crates/app-shell/src/edit_copies.rs`, `crates/app-shell/src/file_clipboard.rs` | Ablage und Aufräumen lokaler Dateien des Dateibrowsers (Spec 0054), vom Command-Layer und vom Start-/Beenden-Ablauf benutzt. |
| `crates/app-shell/src/file_ops.rs` | Laufende Datei-Operationen des erhöhten Kanals; hängt an `ElevatedSftpRegistry` (s. o.) und bleibt deshalb bei ihr. Implementiert `app_logic::sftp_ops::OpProgress` für `OpHandle`. |
| `crates/app-shell/src/startup_gate.rs` | Torliste vor dem `generate_handler!`-Verteiler (Spec 0101 A16); selbst Tauri-frei, gehört zum Tor, das die Verdrahtung einhängt. |
| `crates/app-shell/src/startup_prompt.rs` | Zuordnung Fall → Dialog für die nativen Startdialoge (Spec 0101 A3/A5), arbeitet mit dem nativen Fehlerdialog zusammen. |

Alle übrigen Module in `app-shell` erfüllen die Regel direkt —
`tauri::`/`tauri_plugin_*::`/`AppHandle`/`#[tauri::command]` im eigenen
Code, oder das Paket `rfd` (bei dem Dialogmodul): `android_service`,
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
- `connect` — Host-Key-Entscheidung beim Verbindungsaufbau (`connect::host_key_flow`, Issue #835) und Aufbau der `Session` nach dem Transport: Sonden, Sudo-Passwort, Redactor, System-Prompt, Chat-Verlauf (`connect::session_builder`, Issue #839).
- `auto_reconnect` — automatischer Wiederaufbau; `background_keepalive` — Sitzungen im Hintergrund halten.
- `start_directory` — Startverzeichnis einer Sitzung; `folder_upload` — Plan eines rekursiven Ordner-Uploads.
- `mcp_lookup`, `mcp_sessions` — lesende MCP-Tools und MCP-eigene Sitzungen.
- `ai_provider_factory` — baut den konkreten KI-Provider; `ai_usage` — Erweiterungspunkt Nutzungsbeobachtung; `chat_content_extension` — Erweiterungspunkt für Chat-Inhalte.
  `persist`, `resolve`, `session_removed` und `reconcile` sind `async` (Issue #899), damit eine Erweiterung auf async Speicher (z. B. sqlx) ohne eigene Sync-über-Async-Brücke zugreifen kann; `name`, `source` und `take_pending` bleiben synchron. Eine Implementierung trägt `#[async_trait::async_trait]` und schreibt `async fn`:

  ```rust
  #[async_trait::async_trait]
  impl ChatContentExtension for MyContent {
      fn name(&self) -> &str { "mycontent" }
      async fn persist(&self, session_id: Uuid, part_id: &str, blocks: Vec<RedactedBlock>)
          -> Result<(), ChatContentError> {
          self.store.insert(session_id, part_id, blocks).await.map_err(|e| ChatContentError(e.to_string()))
      }
  }
  ```
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
- `single_instance` — Single-Instance-Entscheidung; `logging` — JSON-Lines-Logging; `version` — Version/Build-Hash; vergiftungsfeste Sperren liegen jetzt in `ssh_manager_core::sync`.
- `test_support` — Testhilfen hinter `test-support` (s. o.).

## Background tasks: `TaskSupervisor` (issue #879)

Long-lived background tasks have an owner. `app_logic::tasks::TaskSupervisor`
(one `Arc` per process, managed by Tauri next to `AppState`) hands out
*scopes* by key. A scope has a cancellation token and a task tracker:

- `scope.spawn(name, fut)` races `fut` against the scope token, so a cancelled
  task stops at its next `.await` and its `Drop` guards run. A pending
  confirmation is therefore removed from the registry exactly as if the
  dialog had been closed — cancellation never becomes an approval.
- `scope.spawn_blocking(name, f)` is tracked and awaited at shutdown, but
  cannot be cancelled.
- `scope.shutdown(timeout)` cancels, closes the tracker and waits up to the
  deadline; names of tasks still running are logged at `warn` (no payloads).
- Spawning into a cancelled scope does not run the future.

**App scope** (`ScopeKey::App`, `background_tasks::app_scope` in app-shell): post-startup tasks
(service host start, MCP autostart, chat retention cleanup, title backfill,
entitlement watch, window decoration) and the on-disconnect and note-shrink AI
requests. The latter deliberately outlive their session. There is no re-lock
today, so the scope ends at exit; a future re-lock cancels the scope without
touching the call sites.

**Session scope** (`ScopeKey::Session`, `TaskSupervisor::session_scope`,
`background_tasks::session_scope`; issue #881): terminal actor, connection
watch, the SFTP tap pumps (injected into the transport as a
`ssh_manager_core::ssh::TaskSpawner` via `SshTransport::set_task_spawner`),
the auto-reconnect run and the MCP `propose_action` task. `CloseContext::close`
calls `close_session_scope(session_id, 2 s)` after the elevated channel is
revoked, the pending confirmation rejected and the transport marked closed:
the scope is cancelled, awaited and removed. Cancelling a task that waits for a
confirmation drops its registry guard — a rejection, never an execution. The
MCP tool call itself is an app-scope task (`McpBackend::spawn_tool_call`); after
an MCP timeout the session-scope action task keeps waiting (Spec 0028 §7) until
the user decides or the session closes. AI usage records go through one bounded
queue and one tracked worker (`ai_usage::start_worker`, app scope); a full
queue drops records with a `warn`.

**Exit order** (`RunEvent::Exit`): `ServiceHost::stop_all()`, then
`TaskSupervisor::shutdown(3 s)`, then the file clipboard cleanup.

Crates below `app-logic` (`speech`, `ssh-transport`, `mcp-server`, `core`)
cannot use the supervisor; their remaining sites sit on the allowlist in
`crates/app-logic/tests/no_untracked_spawns.rs`, which fails on any new direct
`tokio::spawn` / `std::thread::spawn` / `spawn_blocking` in production code and
may only shrink.

## Lock hierarchy of a session (issue #923)

The async locks of one session have a fixed order. A level may be taken while
holding a lower number, never the reverse. `app_logic::session::SessionLocks`
(`Session::locks()`) is the only way to take more than one of them and has no
method for transport-then-SFTP (a `compile_fail` doctest proves it).

| Level | Lock | Protects |
|---|---|---|
| 1 | Elevated-channel slot (`ElevatedSftpRegistry`, `app-shell`) | the elevated SFTP channel of the session |
| 2 | Normal SFTP channel (`SessionLocks::sftp`) | the one lazily opened SFTP session |
| 3 | Transport (`SessionLocks::transport`) | the SSH connection; `begin_command`'s slot serialises AI commands and is taken before level 3 |

The other async locks (`context`, `summary`, chat state, …) are leaves: never
take level 2 or 3 while holding them, and do not take them while holding level
2 (report errors after the SFTP lock is gone).

Rules:

- No `std::sync` guard is held across `.await` (`clippy::await_holding_lock`
  is `deny` in the workspace lints).
- Level 1 is never held together with level 3. Revoking an elevated channel
  happens outside the transport lock, because `disconnect` releases the channel
  lock first and then takes the transport.
- A long command must not keep level 3: use `start_exec` (issue #328) and drop
  the guard while it runs. Only transports without split start keep it for the
  whole command; a disconnect cancels the command first and waits at most 2 s
  per step. The elevation probe works the same way.
- Single-lock sites may use `session.transport.lock()` / `lock_sftp()` directly.
