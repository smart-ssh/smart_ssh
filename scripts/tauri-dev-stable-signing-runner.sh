#!/usr/bin/env bash
# `--runner` für `cargo tauri dev` (Spec 0022, Abschnitt 4) — nicht direkt
# aufrufen, wird von scripts/tauri-dev.sh eingehängt.
#
# Warum ein eigener Runner nötig ist: `bundle.macOS.signingIdentity`/
# `APPLE_SIGNING_IDENTITY` wirken nur auf `tauri build`s Bundler-
# Signierschritt — `tauri dev` läuft auf macOS ohne jeden eigenen
# Signiervorgang schlicht die rohe `cargo run`-Binary, die lediglich die
# vom Rust-Linker automatisch angehängte Ad-hoc-Signatur trägt (nötig,
# damit Apple-Silicon-Binaries überhaupt starten). Diese linker-eigene
# Ad-hoc-Signatur ist inhaltsbasiert und ändert sich bei jedem Neubau —
# genau das ursprüngliche Problem. `tauri-cli` ruft einen konfigurierten
# `--runner` exakt wie `cargo` selbst auf (`<runner> run [cargo-args] --
# [app-args]`, empirisch verifiziert) — dieses Skript nutzt das, um
# zwischen "cargo hat fertig gebaut" und "die Binary läuft" einen eigenen
# `codesign`-Schritt mit stabiler Identität einzuschieben. Details:
# docs/adr/0022-stable-dev-code-signature.md.

set -euo pipefail

# Alles außer `run` unverändert an das echte `cargo` durchreichen (z. B.
# `cargo metadata`-Aufrufe, die `tauri-cli` intern selbst macht).
if [[ "${1:-}" != "run" ]]; then
  exec cargo "$@"
fi
shift

cargo_args=()
app_args=()
seen_separator=false
for arg in "$@"; do
  if [[ "$seen_separator" == false && "$arg" == "--" ]]; then
    seen_separator=true
    continue
  fi
  if [[ "$seen_separator" == true ]]; then
    app_args+=("$arg")
  else
    cargo_args+=("$arg")
  fi
done

# `${arr[@]+"${arr[@]}"}` statt `${arr[@]}`: macOS liefert standardmäßig
# Bash 3.2 aus (letzte GPLv2-Version), die unter `set -u` bei einem *leeren*
# Array (kein einziges Element per `+=` zugewiesen) fälschlich "unbound
# variable" wirft — dieses Muster expandiert dann korrekt zu nichts statt
# abzubrechen.
#
# `--message-format=json` liefert u. a. eine Zeile pro erzeugter Binary mit
# einem `"executable":"<pfad>"`-Feld — reines `grep`/`cut` statt einer
# `jq`-Abhängigkeit, robust genug für dieses einzelne, feste Bin-Target.
binary_path="$(
  cargo build "${cargo_args[@]+"${cargo_args[@]}"}" --message-format=json-render-diagnostics \
    | grep -o '"executable":"[^"]*"' \
    | tail -n1 \
    | cut -d'"' -f4
)"

if [[ -z "$binary_path" ]]; then
  echo "tauri-dev-stable-signing-runner: konnte den Pfad der gebauten Binary nicht ermitteln" >&2
  exit 1
fi

if [[ "$(uname)" == "Darwin" ]]; then
  # Muss exakt mit CERT_NAME in setup-macos-dev-signing.sh übereinstimmen.
  codesign --force --sign "Smart SSH Dev Signing" "$binary_path"
fi

exec "$binary_path" "${app_args[@]+"${app_args[@]}"}"
