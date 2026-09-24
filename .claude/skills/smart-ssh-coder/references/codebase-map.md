# Codebase-Karte und Erweiterungs-Fallstricke

Stand bei Anlage dieses Skills — vor Nutzung per `grep`/`ls` verifizieren,
Namen können sich ändern.

## Backend

| Ort | Inhalt |
|---|---|
| `crates/core/src/ai/types.rs` | `AiEvent` (TextDelta, ActionProposed, Done, TextTruncated, Error), `AiError`, `SessionContext` (inkl. `max_tokens_hint`), Aktions-Schemas |
| `crates/core/src/ai/` | Redactor, Fence-Marker |
| `crates/core/src/filter/`, `risk/` | Filter-Engine, Risiko-Klassifizierer (sicherheitskritisch, Tests in `filter/tests.rs`) |
| `crates/core/src/ssh/` | Traits `SshTransport`, `SftpSession`; `elevated.rs` (sudo/sftp-server-Logik); `mock.rs` (`MockSftpSession`, Feature `test-support`) |
| `crates/core/src/profiles/types.rs` | `Server`, `AiAction`, `AuthMethod` … |
| `crates/ai-providers/src/` | `anthropic.rs`, `openai_compatible.rs` (Stream-Parsing, Tool-Call-Zurückhalten, Retry-Wrapper), `retry.rs` (429), Rate-Limit-Budget |
| `crates/ai-providers/tests/` | wiremock-Integrationstests pro Provider |
| `crates/app-shell/src/orchestration.rs` | `run_chat_turn`, `run_one_round` (→ `RoundOutcome`), `handle_action_proposed`, `execute_*`, `push_history`, großer Testblock am Ende |
| `crates/app-shell/src/commands.rs` | alle Tauri-Commands: `send_chat_message_impl` (Warteschlange, Turn-Start), `build_session_system_context` (System-Prompt), `stop_auto_continuation`, `sftp_*` (Browser, `BrowserChannel`, `BrowserAccess`), `sftp_elevation_*`, `get_app_info` |
| `crates/app-shell/src/session.rs` | `Session` (alle Session-Felder), `ChatTurnState`, Stopp-Hilfen |
| `crates/app-shell/src/events.rs` | `emit_*`-Funktionen (Event-Namen kebab-case) |
| `crates/app-shell/src/dto.rs` | DTOs zum Frontend (`#[serde(rename_all = "camelCase")]`) |
| `crates/app-shell/src/elevated_sftp.rs` | erhöhter SFTP-Kanal (Slot, enable/disable) |
| `crates/app-shell/src/test_support.rs` | In-Memory-Stores, `session_with_transport(...)` |
| `crates/app-shell/src/version.rs` | Build-Hash, `BuildType` (Dev/Release) |
| `crates/persistence-sqlite/migrations/` | `NNNN_*.sql`, fortlaufend |
| `crates/ssh-transport/src/transport.rs` | russh-Transport (`open_sftp`, `open_sftp_via_exec`) |
| `crates/ssh-transport/tests/` | echte SSH-Tests gegen einen In-Prozess-russh-Server (`fixtures/test_server.rs`) |

## Frontend (`apps/smart-ssh-community/frontend/src`)

| Ort | Inhalt |
|---|---|
| `api.ts` | `invoke`-Wrapper für jeden Tauri-Command |
| `events.ts`, `types.ts` | Event-Listener, DTO-Typen |
| `components/ChatPanel.tsx` | Chat, Stopp-Knopf, Warteschlange (`pendingSendsRef`), Hinweise |
| `components/FileBrowserPanel.tsx` | Dateibrowser, Aktionen, erhöhter Modus |
| `useLocalEditSession.ts` | "Lokal öffnen"-Flow |
| `toastBus.ts` + `components/ToastHost.tsx` | app-weite Erfolgs-/Fehlermeldungen (`showToast`) |
| `locales/{de,en}/common.json` | Texte; neue Texte immer in beiden Sprachen |

## Fallstricke beim Erweitern

- **Neues `Session`-Feld**: Die Konstruktionsstellen nicht aus dem
  Gedächtnis zählen, sondern auflisten lassen:
  ```bash
  grep -rnE '\bSession \{' --include='*.rs' crates apps | grep -v -e '->' -e 'struct ' -e 'impl '
  ```
  Liefert die Struct-Literale (produktiv in `commands.rs`, dazu Tests und
  `test_support`) — ohne Signaturen und ohne andere Typen. Jede Stelle
  bekommt das Feld, danach `cargo check --workspace --all-targets`.
- **Neues `Server`-Profilfeld**: viele Struct-Literale über mehrere Crates
  (auch Examples, Tests, `ServerInput`-Literale). Hat das Feld keinen
  Default, zeigt `cargo check --workspace --all-targets` jede fehlende
  Stelle — eingefügt wird jeweils **hinter dem letzten Feld** des Literals,
  nicht hinter einem namentlich genannten (die Reihenfolge ändert sich mit
  jeder Migration). Dazu Migration, `store.rs` (beide SELECTs, INSERT, das
  vollständige UPDATE, `row_to_server` — auflisten mit
  `grep -nE 'FROM servers|INSERT INTO servers|UPDATE servers|fn row_to_server' crates/persistence-sqlite/src/store.rs`), DTO,
  Frontend-Typ + Test-Fixtures. Der Migrationstest
  (`test_migration_from_earlier_schema_…`) legt Daten im **alten** Schema
  per rohem SQL an — neue Spalten dürfen dort nicht vorkommen.
- **Neuer Parameter an einem Tauri-Command**: fehlt er beim `invoke`,
  scheitert jeder Aufruf → `api.ts`-Wrapper sofort mitziehen (Default-
  Wert), Test-Erwartungen `toHaveBeenCalledWith(...)` anpassen.
- **Neue API-/Event-Funktion im Frontend**: in allen Tests, die `../api`
  bzw. `../events` per `vi.mock` ersetzen, ergänzen (sonst
  `undefined is not a function`); Mocks, auf die `.catch` folgt, müssen
  ein Promise liefern.
- **Neuer `AiEvent`-Variant**: alle `match event`-Stellen (Hauptrunde +
  Nebenaufrufe in orchestration/compaction/risk_second_opinion) bekommen
  einen Arm.
- **Nebenaufrufe an die KI** (Titel, Notiz, Zusammenfassung,
  Zweitmeinung, Injection-Check) setzen `max_tokens_hint =
  Some(SIDE_CALL_MAX_TOKENS)` — nie den großen Hauptchat-Default erben.
- `cargo fmt` bricht Code um → Python-Replaces auf frisch formatierten Code
  können danach nicht mehr greifen; bei `AssertionError` erst den
  aktuellen Stand lesen.
- `rustfmt`/clippy: `needless_borrow` tritt auf, wenn eine Hilfsfunktion
  schon `&Session` hat und nochmal `&session` übergeben wird.
