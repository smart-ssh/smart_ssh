#!/usr/bin/env bash
# Wrapper um `cargo tauri dev`, der auf macOS eine stabile, projekteigene
# Code-Signatur statt der bei jedem Build neu berechneten Ad-hoc-Signatur
# des Rust-Linkers erzwingt (Spec 0022, Abschnitt 4) — s.
# docs/adr/0022-stable-dev-code-signature.md für die vollständige
# Begründung, insbesondere warum das über einen eigenen `--runner`
# (scripts/tauri-dev-stable-signing-runner.sh) läuft statt über
# `bundle.macOS.signingIdentity`/`APPLE_SIGNING_IDENTITY`: Letztere wirken
# nachweislich nur auf `tauri build`s Bundler-Signierschritt, nicht auf
# `tauri dev`, das die rohe `cargo run`-Binary ohne jeden eigenen
# Signiervorgang startet.
#
# Bewusst als eigenes Skript statt fest in tauri.conf.json verankert: der
# `--runner`/das Zertifikat existieren nur lokal auf Entwickler-Maschinen —
# in der Projekt-Konfiguration verankert würde es auch für `tauri build`
# gelten und den Release-Workflow auf GitHub Actions brechen (dort
# existiert dieses Zertifikat nicht).
#
# Nutzung: ./scripts/tauri-dev.sh   (statt `cargo tauri dev` direkt)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$REPO_ROOT/apps/smart-ssh-community"

cd "$APP_DIR"

if [[ "$(uname)" == "Darwin" ]]; then
  "$SCRIPT_DIR/setup-macos-dev-signing.sh"
  exec cargo tauri dev --runner "$SCRIPT_DIR/tauri-dev-stable-signing-runner.sh" "$@"
fi

exec cargo tauri dev "$@"
