# ADR 0076 — Entscheidungen bei der Modul-Aufteilung von `orchestration.rs`/`commands.rs` (Spec 0083)

Status: angenommen · 2026-09-27 · Spec: `docs/specs/0083-app-shell-module-aufteilen.md`
Betrifft: `crates/app-shell/src/orchestration.rs` + `orchestration/`,
`crates/app-shell/src/commands.rs` + `commands/`

## 1. Glob-Re-Export statt geänderter Pfade in `generate_handler!`

Spec 0083 §1 ließ beides offen ("nachgelesen, nicht kompiliert"): entweder
jedes Modul per `pub(crate) use submodul::*;` an der Wurzel re-exportieren,
oder die Pfade in `lib.rs`s `generate_handler!` direkt auf die neuen
Submodul-Pfade ändern (A3.4 erlaubt beides).

Entschieden für Glob-Re-Export (`commands.rs`: zwölf
`pub(crate) use <modul>::*;`-Zeilen). Ergebnis nach dem Kompilieren: `cargo
check` läuft ohne jede Änderung an `lib.rs` durch — die von
`#[tauri::command]` je Funktion erzeugten Hilfsmakros (`__cmd__<name>` u. a.)
werden vom Glob mitgenommen, ein benannter Re-Export hätte sie nicht
mitgebracht (Spec-Ausgangslage, §1). `git diff -- crates/app-shell/src/lib.rs`
ist entsprechend leer (T4). Dieselbe Technik trägt auch
`crate::commands::BrowserAccess`/`SSH_CONNECT_TIMEOUT`/
`sanitize_uname_output`/`build_os_banner_message`, die von außerhalb von
`commands` (u. a. `elevated_sftp.rs`, `orchestration/chat_turn/tests_*.rs`)
weiterhin unter ihrem alten `crate::commands::…`-Pfad erreicht werden.

Eine Einschränkung dabei beobachtet und für künftige Aufteilungen
festgehalten: ein `pub(super)`-Element eines Submoduls wird von einem
Glob-Re-Export an der Wurzel **nicht** über die Wurzel hinaus sichtbar,
selbst wenn die Re-Export-Zeile selbst `pub(crate)` deklariert — Rust
lässt eine Glob-Reexport-Zeile ein einzelnes Element nur bis zu dessen
eigener deklarierter Sichtbarkeit passieren, nie darüber hinaus erweitern.
Für die zwei betroffenen Fälle (`sanitize_uname_output`,
`build_os_banner_message`, extern gebraucht von
`orchestration/chat_turn/tests_rounds.rs`) blieb deshalb die ursprüngliche
Sichtbarkeit `pub(crate)` (unverändert gegenüber der alten Datei) statt
der sonst für kommandointerne Aufrufe verwendeten `pub(super)` — kein
Widerspruch zu A3.2, da `pub(crate)` bereits die Sichtbarkeit vor der
Aufteilung war.

## 2. Schnitt von `commands.rs`

Zwölf Themenmodule:
`servers`, `groups`, `ai_providers`, `connect`, `terminal`, `chat`, `notes`,
`rules`, `diagnostics_export`, `app_meta`, `elevation`, `sftp`, dazu ein
gemeinsames `#[cfg(test)]`-Hilfsmodul `test_support` (ein `dummy_server`-
Fixture, das `servers`- und `connect`-Tests teilen — analog zu
`orchestration/test_support.rs`). Zwei ursprünglich benachbarte
Alt-Testcontainer wurden inhaltlich aufgeteilt, weil sie Funktionen aus
zwei verschiedenen neuen Modulen zusammen testeten (T2 vergleicht Elemente,
nicht Container, das ist zulässig, s. Spec 0083 §6 T2): aus
`send_chat_message_persistence_tests` wanderten zwei Host-Key-Wait-Tests
nach `connect::host_key_wait_tests`; aus `edit_session_tests` wanderte ein
einzelner `build_os_banner_message`-Test nach
`diagnostics_export::banner_message_tests`. `local_server_tests` blieb als
Ganzes in `servers.rs`, obwohl einige seiner `build_session_system_context`-
Tests inhaltlich `connect.rs` treffen — sie testen überwiegend den lokalen
Pseudo-Server-Pfad (`LOCAL_SERVER_ID`), der Name passt.

## 3. Sichtbarkeits-Erweiterungen (A3.2)

Von privat auf `pub(super)` angehoben, weil ein Geschwistermodul unter
`commands` sie jetzt braucht (Liste nicht erschöpfend):
`ai_providers::active_ai_provider_config`,
`servers::resolve_server_for_note_shrink`,
`connect::build_session_system_context`, `sftp::edit_session_dir`,
`elevation::{BrowserChannel, BrowserSftpGuard, lock_browser_sftp,
browser_session, file_name_of, safe_local_segment, audit_elevated_change,
write_local_download}`. Keine Erweiterung geht über `pub(crate)` hinaus;
die öffentliche Schnittstelle des Crates (`app_shell::run`,
`app_shell::Wiring`, `app_shell::Edition`) ist unverändert (T4).
