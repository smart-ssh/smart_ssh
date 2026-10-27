#!/usr/bin/env bash
# The one definition of the gate. Contributors and CI (community.yml, `test`
# job) run the same stages from here.
#
# Usage: scripts/gate.sh [--ci] [rust|boundary|frontend ...]
#   no stage  = all stages, in this order
#   --ci      = CI mode: `npm ci` always, SMART_SSH_REQUIRE_CLIPBOARD_TESTS=1,
#               and on Linux `cargo test` runs under `xvfb-run -a`
#
# Every Cargo build/test/clippy/tree call uses `--locked`: the checked-in
# Cargo.lock is what gets tested, a stale lockfile fails instead of being
# re-resolved silently.
#
# Windows CI keeps its Cargo steps inline in the default shell (Git Bash's perl
# cannot build the vendored OpenSSL); keep those commands identical to rust below.
set -euo pipefail

usage() {
  echo "usage: scripts/gate.sh [--ci] [rust|boundary|frontend ...]" >&2
}

cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT="$(pwd)"

ci=0
stages=()
for arg in "$@"; do
  case "$arg" in
    --ci) ci=1 ;;
    rust|boundary|frontend) stages+=("$arg") ;;
    *) echo "unknown argument: $arg" >&2; usage; exit 2 ;;
  esac
done
if [ ${#stages[@]} -eq 0 ]; then
  stages=(rust boundary frontend)
fi

step() {
  echo
  echo "=== $* ==="
}

stage_rust() {
  step "cargo fmt --all --check"
  cargo fmt --all --check
  step "cargo clippy"
  cargo clippy --workspace --all-targets --locked -- -D warnings
  step "cargo test"
  # Issue #358: the clipboard round-trip test needs a display and must not be
  # skipped in CI.
  if [ "$ci" -eq 1 ]; then
    export SMART_SSH_REQUIRE_CLIPBOARD_TESTS=1
    if [ "$(uname -s)" = "Linux" ]; then
      if ! command -v xvfb-run >/dev/null 2>&1; then
        echo "::error::xvfb-run is missing: the clipboard test needs a virtual X server in --ci mode (install xvfb)"
        exit 1
      fi
      xvfb-run -a cargo test --workspace --locked --no-fail-fast
    else
      cargo test --workspace --locked --no-fail-fast
    fi
  else
    cargo test --workspace --locked --no-fail-fast
  fi
  step "cargo build (without test-support)"
  cargo build --workspace --locked
}

# Spec 0084, A3/T4: `app-logic` must not contain tauri/tauri-*/wry/tao/rfd in
# its whole dependency tree (any kind, any feature). `-p app-logic`: the tree of
# `app-shell` itself contains `tauri` by design. `--prefix none` gives one
# `name vX.Y.Z` per line so `^` anchors on the package name. The tree goes into
# a variable first so a failing `cargo tree` has its own explicit error path
# (a pipeline into grep would let it pass silently).
stage_boundary() {
  step "app-logic is Tauri-free (Spec 0084, A3)"
  local tree_output
  if ! tree_output="$(cargo tree -p app-logic -e normal,build,dev --all-features --locked --prefix none)"; then
    echo "::error::cargo tree -p app-logic failed - boundary check (Spec 0084 A3) could not run"
    exit 1
  fi
  if echo "$tree_output" | grep -E '^(tauri|tauri-[a-zA-Z0-9_-]*|wry|tao|rfd)( |$)'; then
    echo "::error::app-logic depends on tauri/tauri-*/wry/tao/rfd (Spec 0084, A3) - see line(s) above"
    exit 1
  fi
}

stage_frontend() {
  cd "$ROOT/apps/smart-ssh-community/frontend"
  if [ "$ci" -eq 1 ] || [ ! -d node_modules ]; then
    step "npm ci"
    npm ci
  fi
  step "tsc -b"
  npx tsc -b
  step "npm run lint"
  npm run lint
  step "vitest"
  npx vitest run
  step "npm run build"
  npm run build
  cd "$ROOT"
}

ran=()
for s in "${stages[@]}"; do
  "stage_$s"
  ran+=("$s")
done

echo
echo "GATE GREEN: stages ran: ${ran[*]}"
