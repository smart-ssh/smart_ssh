# ADR 0077 — Entscheidungen beim Auslagern von `app-logic` (Spec 0084)

Status: angenommen · 2026-09-28 · Spec: `docs/specs/0084-app-logic-crate.md`
Betrifft: `crates/app-shell/`, neu `crates/app-logic/`

## 1. Verworfene Wege (Spec §4, letzter Punkt)

- **Zugangsnachweis (`BrowserAccess`) nach `app-logic` mit öffentlichem
  Konstruktor plus CI-Prüfung.** Verworfen: Die Garantie aus Spec 0067 A
  hinge dann an einem Skript statt am Compiler — genau das Gegenteil des
  Ziels dieser Spec. `BrowserAccess`, `ElevatedSftpRegistry`,
  `ElevatedSftp`, `ElevationContext` bleiben deshalb vollständig in
  `app-shell` (`elevated_sftp.rs`, `commands::elevation`), obwohl sie
  selbst Tauri-frei sind.
- **Nur die Tauri-freien Module auslagern, alles andere unangetastet
  lassen.** Verworfen: hätte das Ziel (A2) verfehlt — mehrere Module waren
  nur über eine einzelne Konstante/einen einzelnen Typ an ein
  Tauri-gebundenes Modul gekoppelt (§1), und ohne die in §4 benannten
  Schnitte hätte `app-logic` transitiv von `app-shell` abhängen müssen.

## 2. Zusätzliche Kopplungen, die §1 nicht maß

§1 wurde am Stand `1286046` (vor Spec-0084-Teil-1) gemessen. Zwei
Kopplungen kamen dadurch nicht in die Liste, wurden aber beim Umsetzen von
Schritt b sichtbar:

- **`error.rs`** (Fehlertyp `CommandResult`/`CommandError`) — Tauri-frei,
  aber von praktisch jedem Modul auf beiden Seiten der Grenze gebraucht.
  Zieht nach `app-logic`, `pub`.
- **`version.rs`** trägt `env!("SMART_SSH_BUILD_HASH")`, gesetzt von
  `build.rs` — Cargo isoliert `cargo:rustc-env` strikt auf die Crate, die
  das Build-Skript ausführt. Da `version.rs` nach `app-logic` zieht, zieht
  `build.rs` mit; `app-shell` hat seither kein eigenes Build-Skript mehr
  (nichts dort liest die Variable noch direkt). Verhalten unverändert
  (derselbe Hash, dieselbe `"unknown"`-Rückfallregel ohne Git).

Beide sind reine Verschiebungen ohne Inhaltsänderung (A7) — die
Anpassungen in `build.rs`s Kommentaren nennen nur den neuen Crate-Namen.

## 3. `risk_second_opinion`-Schnitt: neues Modul `second_opinion`

§4 beschreibt den Schnitt konzeptionell ("die Abruf-Logik zieht nach
`app-logic`"), nennt aber keinen Zielnamen — er existierte vorher nicht
als eigenes Modul. Entschieden für `app-logic::second_opinion` (neu):
`fetch_second_opinion`, `fetch_injection_check`, beide Prompt-Konstanten,
`parse_second_opinion`/`parse_injection_check`/`parse_escalating_verdict`
und deren komplette Testsuite (inkl. der Monotonie-Eigenschaftstests aus
Spec 0074) ziehen dorthin. `app-shell::risk_second_opinion` behält nur
`resolve_second_opinion_provider` (liest `tauri_plugin_store`-
Einstellungen, baut den `AiProvider`) — beide Module kennen sich nicht
gegenseitig, `action_exec` (in `app-logic`) bekommt den fertigen Provider
als Wert.

## 4. `sanitize_uname_output`: Zielort `orchestration`

§4 sagt nur "zieht nach `app-logic`, wenn Tauri-frei" — es ist Tauri-frei
(pure Zeichen-Validierung). Zielort innerhalb von `app-logic`: die Wurzel
von `orchestration.rs`, weil der einzige Cross-Modul-Nutzer außerhalb von
`commands::connect` ein Test in `orchestration/chat_turn` ist (Spec 0013,
SEC-02, T5) und die Funktion inhaltlich zur Prompt-Zusammensetzung gehört,
für die `orchestration` bereits die Heimat ist. `build_os_banner_message`
(dieselbe Nachbarschaft im Ursprungsmodul, aber kein Kopplungs-Kandidat
nach §1 — sein einziger Aufrufer ist `commands::connect`) bleibt
unverändert in `app-shell::commands::diagnostics_export`.

## 5. `test_support.rs`-Schnitt (A5) — was gemeinsam liegt und was nicht

Das bisherige `app-shell::test_support` bediente drei verschiedene
Bedürfnisse, die die Spec nicht einzeln benennt:

- `InMemoryProfileStore`/`InMemoryCredentialStore`/`session_with_transport`/
  `log_capture`/`policy::NoRulesPolicyStore` — gebraucht von Tests auf
  **beiden** Seiten der Grenze (u. a. `app-logic::identity_file`/
  `server_credentials`/`test_connection`/`dto`/`orchestration::*` **und**
  `app-shell::commands::{chat, servers, ai_providers}`,
  `ssh_config_apply/tests.rs`). Ziehen nach `app_logic::test_support`
  (bzw. `app_logic::policy`) hinter `#[cfg(any(test, feature =
  "test-support"))]` — dasselbe Muster wie `ssh_manager_core::ssh::mock`.
  `session_with_transport` bekam dabei eine Geschwisterfunktion
  `session_with_ai_and_transport` (frei wählbarer `AiProvider` statt
  fest `NoAi`) — ohne sie hätte T5/T5b (Punkt 6) keinen Weg gehabt, eine
  KI-/MCP-Aktion zu simulieren.
- `elevation` (Zugriffs-Helfer für den erhöhten Kanal, `interleave_hook`)
  — bleibt wörtlich in `app-shell::test_support`, unverändert bis auf den
  einen cross-crate-Aufruf von `session_with_transport` (Auftrags-Vorgabe,
  deckungsgleich mit A1).
- `MockAiProvider` (die **oberste**, undifferenzierte Variante — nicht zu
  verwechseln mit der spezialisierten, `pub(crate)`-Variante in
  `orchestration/test_support.rs`, die unangetastet mit `orchestration`
  zieht) — nur von `app-shell::commands::ai_providers`s eigenem Test
  gebraucht. Bleibt in `app-shell::test_support`, nicht hinter
  `test-support` verschoben — A5 verlangt die Cross-Crate-Sichtbarkeit nur
  für Helfer, die **beide** Crates brauchen.

`orchestration::chat_turn`/`::action_exec`/`::notes`/`::remote_files`
brauchten für einzelne Re-Exports eine Sichtbarkeits-Anhebung von
`pub(crate)` auf `pub` (A6.4): `push_history`,
`PENDING_ACTION_CONFIRM_TIMEOUT`, `handle_mcp_action_proposed`,
`propose_note_from_chat_content`, `LARGE_NOTE_DIALOG_THRESHOLD_CHARS`,
`ensure_sftp_open`, `sanitize_uname_output`,
`session::history_contains_untrusted_content`,
`compaction::{SystemContextParts, RollingSummary,
model_context_window_tokens, round_count}`,
`test_connection::SSH_CONNECT_TIMEOUT`,
`ai_provider_factory::DEFAULT_ANTHROPIC_BASE_URL`,
`confirmation::ConfirmationRegistry::contains` (letztere zusätzlich von
`#[cfg(test)]` auf `#[cfg(any(test, feature = "test-support"))]`, weil
`app-shell::commands::connect`s Test sie braucht) — jeweils, weil ein
weiterhin in `app-shell` verbliebenes Modul sie direkt aufruft.
`ssh_config_apply::PendingImport`/`local_server::{LOCAL_SERVER_ID,
is_local}` sind die beiden in Spec §4 ausdrücklich benannten Schnitte;
ihre Zielorte (`state.rs` bzw. `dto.rs`) folgen aus §1s Beschreibung, wer
sie als Tauri-freier Nutzer braucht.

## 6. Zwei Tests wandern von `orchestration` nach `app-shell`

`test_ai_and_mcp_file_actions_never_use_the_elevated_channel` (T5, Spec
0067 A5) und `test_mcp_write_actions_never_use_the_elevated_channel` (T5b,
aus Teil 1) bauen den erhöhten Kanal direkt auf
(`ElevatedSftpRegistry::insert_for_tests`, `BrowserAccess::for_tests`) —
beide Typen bleiben nach A1 in `app-shell`. Da `orchestration` komplett
nach `app-logic` zieht, können diese beiden Tests dort nicht bleiben, ohne
`elevated_sftp`/`commands::BrowserAccess` aus `app-logic` heraus sichtbar
zu machen — das wäre A1 direkt zuwidergelaufen. Beide Tests zogen deshalb
(wörtlich gleicher Name, A6.3) nach
`app-shell::commands::elevation::browser_channel_tests`, in dieselbe
Nachbarschaft wie die übrigen T6–T10-Tests aus Teil 1. Eine lokale Kopie
von `AllowEverythingPolicyStore` (zehn Zeilen, kein Delegieren an
`orchestration::test_support`s `pub(crate)`-Variante) hält den
Cross-Crate-Sichtbarkeitszuwachs auf das Nötigste beschränkt.

## 7. `app-logic`s Cargo-Abhängigkeiten

Übernommen aus `app-shell/Cargo.toml`, abzüglich `tauri`/`tauri-plugin-*`/
`rfd` (A3) und `directories`/`libc`/`regex`/`sha2`/`tracing-subscriber`
(in `app-shell` nach der Verschiebung ungenutzt geblieben, dort entfernt —
`tracing-appender` bleibt dort für den `WorkerGuard`-Rückgabetyp).
`tracing-subscriber` zieht komplett nach `app-logic` und in die normalen
(nicht Dev-)Abhängigkeiten, weil `logging.rs`s Produktiv-Subscriber-Aufbau
mitzieht — anders als bei `ssh-manager-core`/`ssh-transport`, deren
`test-support`-Feature rein testbezogen ist.
