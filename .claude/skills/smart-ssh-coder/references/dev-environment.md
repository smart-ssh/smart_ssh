# Dev-Umgebung, Builds, Daten

## Builds

- **Dev**: `./scripts/tauri-dev.sh` aus dem Repo-Root (nie plain
  `cargo tauri dev`, s. ADR 0022). Im Hintergrund starten und per
  `Monitor` auf `replacing existing signature` (App gestartet) bzw.
  Fehlermeldungen warten.
  - Frontend-Änderungen laden per Vite-HMR ohne Neustart.
  - **Rust-Änderungen lösen Rebuild + Fensterneustart aus** — nicht
    während Stefan testet; stattdessen sammeln und nach seinem OK machen.
  - Nur einmal am Ende einer Aufgabe starten, nicht wiederholt.
- **Release (lokal)**: `cargo tauri build` in `apps/smart-ssh-community`
  → `target/release/bundle/macos/Smart SSH.app` (+ `.dmg`).
- Beenden der Dev-App: Prozesse `target/debug/smart-ssh`,
  `tauri-dev-stable-signing-runner`, `cargo-tauri tauri dev`, `vite`
  (Exit 143 danach ist erwartet).

## Toolchain-Stolpersteine (Stefans Mac, Apple Silicon)

- `cargo-tauri` **muss nativ arm64 sein** (`file ~/.cargo/bin/cargo-tauri`).
  Eine x86_64-Version läuft ohne Rosetta nicht ("Bad CPU type") und hat
  unter Rosetta Link-/SDK-Fehler verursacht (auch ein `_x64.dmg`-Name).
  Neu installieren: `cargo install tauri-cli --version <x> --locked`.
- Scheitert das Linken mit `unknown architecture arm64e.x1-macos` /
  `tapi error: malformed file` im `MacOSX27.0.sdk`, half ein SDK-Pin:
  `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk`
  vor den Build-Befehl. Erst prüfen, ob es ohne geht.
- macOS hat kein `timeout`-Binary — Zeitgrenzen in Tests selbst setzen.

## Datenverzeichnisse (`crates/persistence-sqlite/src/paths.rs`)

- Release: `~/Library/Application Support/Smart SSH/smart-ssh.db`
- Dev (Debug-Build): `~/Library/Application Support/Smart SSH (dev)/smart-ssh.db`
- Override für beide: `SMART_SSH_DATA_DIR`.
- Der Über-Dialog zeigt "Dev-Build"/"Release-Build", die Titelzeile
  "· Dev" — bei "meine Server sind weg" zuerst prüfen, welcher Build
  läuft.
- **DB kopieren nie mit `cp`, solange eine App sie offen hat** (WAL →
  "database disk image is malformed"). Stattdessen:
  `sqlite3 "<quelle>" ".backup '<ziel>'"`, danach
  `PRAGMA integrity_check;` auf dem Ziel. Vorher das Ziel sichern.

## Sonstiges

- Temporäre Dateien in den Scratchpad der Sitzung, nicht nach `/tmp`.
- Test-Gegenbeweise: Backup der Datei in den Scratchpad, nach dem Test
  zurückkopieren und mit `grep -c` prüfen, dass der Fix wieder drin ist.
