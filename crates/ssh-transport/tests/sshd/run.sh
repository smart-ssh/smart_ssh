#!/usr/bin/env bash
# End-to-end runner: builds the sshd images, generates throw-away keys, starts
# every scenario under scenarios/, waits for the banners and runs the
# `#[ignore]` tests of every tests/sshd_*.rs target. See README.md.
# Usage from the repo root: crates/ssh-transport/tests/sshd/run.sh
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
run_id="$$"
prefix="sshe2e${run_id}"
keys_dir="$(mktemp -d)"
declare -a containers=() networks=()

log() { echo "[sshd-e2e] $*" >&2; }

cleanup() {
  local rc=$?
  for c in "${containers[@]:-}"; do
    [ -n "$c" ] && docker rm -f "$c" >/dev/null 2>&1
  done
  for n in "${networks[@]:-}"; do
    [ -n "$n" ] && docker network rm "$n" >/dev/null 2>&1
  done
  rm -rf "$keys_dir"
  exit "$rc"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Reads `key = value` from a scenario.toml; strings lose their quotes.
toml_get() {
  sed -n "s/^[[:space:]]*$2[[:space:]]*=[[:space:]]*//p" "$1" | head -n1 | sed 's/[[:space:]]*#.*$//; s/^"\(.*\)"$/\1/'
}
# Array of strings: networks = ["a", "b"] -> one name per line.
toml_list() {
  toml_get "$1" "$2" | sed 's/^\[//; s/\]$//' | tr ',' '\n' | sed 's/^[[:space:]]*"\{0,1\}//; s/"\{0,1\}[[:space:]]*$//' | sed '/^$/d'
}
env_name() { echo "$1" | tr 'a-z-' 'A-Z_'; }

# --- keys: generated per run, never committed ---------------------------
log "generating test keys in a temporary directory"
ssh-keygen -q -t ed25519 -N '' -C test-ca -f "$keys_dir/ca" || exit 1
ssh-keygen -q -t ed25519 -N '' -C test-user -f "$keys_dir/user_ed25519" || exit 1
ssh-keygen -q -t rsa -b 2048 -N '' -C test-user-rsa -f "$keys_dir/user_rsa" || exit 1
ssh-keygen -q -s "$keys_dir/ca" -I testuser -n testuser -V -5m:+1d "$keys_dir/user_ed25519.pub" || exit 1

# --- images ----------------------------------------------------------------
for img in "$here"/images/*/; do
  name="$(basename "$img")"
  log "building image $name"
  docker build -q -t "smart-ssh-e2e-$name" "$img" >/dev/null || { log "image build failed: $name"; exit 1; }
done

# --- scenarios ---------------------------------------------------------------
declare -a scenario_names=()
declare -A exports=()
for dir in "$here"/scenarios/*/; do
  name="$(basename "$dir")"
  toml="$dir/scenario.toml"
  [ -f "$toml" ] || { log "scenario $name: scenario.toml missing"; exit 1; }
  image="$(toml_get "$toml" image)"
  host="$(toml_get "$toml" hostname)"
  publish="$(toml_get "$toml" publish)"
  [ -d "$here/images/$image" ] || { log "scenario $name: unknown image '$image'"; exit 1; }
  [ -n "$host" ] || host="$name"
  mapfile -t nets < <(toml_list "$toml" networks)
  [ "${#nets[@]}" -gt 0 ] || nets=(default)

  for n in "${nets[@]}"; do
    net="$prefix-$n"
    if ! printf '%s\n' "${networks[@]:-}" | grep -qx "$net"; then
      docker network create "$net" >/dev/null || { log "scenario $name: cannot create network $n"; exit 1; }
      networks+=("$net")
    fi
  done

  cname="$prefix-$name"
  args=(-d --name "$cname" --hostname "$host" --network "$prefix-${nets[0]}" --network-alias "$host"
        -v "$keys_dir:/keys:ro" -v "$dir:/scenario:ro")
  [ "$publish" = "true" ] && args+=(-p "127.0.0.1::22")
  containers+=("$cname")
  docker run "${args[@]}" "smart-ssh-e2e-$image" >/dev/null || { log "scenario $name: container start failed"; exit 1; }
  for n in "${nets[@]:1}"; do
    docker network connect --alias "$host" "$prefix-$n" "$cname" || { log "scenario $name: cannot join network $n"; exit 1; }
  done
  scenario_names+=("$name")
  exports["SSHD_$(env_name "$name")_CONTAINER"]="$cname"
  if [ "$publish" = "true" ]; then
    port="$(docker port "$cname" 22/tcp | head -n1 | sed 's/.*://')"
    [ -n "$port" ] || { log "scenario $name: no published port"; exit 1; }
    exports["SSHD_$(env_name "$name")_PORT"]="$port"
  fi
done

# --- wait for banners (30 s per scenario) -----------------------------------
for name in "${scenario_names[@]}"; do
  cname="$prefix-$name"
  port="${exports[SSHD_$(env_name "$name")_PORT]:-}"
  ok=0
  for _ in $(seq 1 30); do
    if [ "$(docker inspect -f '{{.State.Running}}' "$cname" 2>/dev/null)" != "true" ]; then
      break
    fi
    if [ -z "$port" ]; then
      # Not published: ask from inside the container.
      docker exec "$cname" sh -c 'echo | nc -w 2 127.0.0.1 22 2>/dev/null | head -c 4' 2>/dev/null | grep -q SSH- && { ok=1; break; }
    elif timeout 2 bash -c "exec 3<>/dev/tcp/127.0.0.1/$port && head -c 4 <&3" 2>/dev/null | grep -q SSH-; then
      ok=1; break
    fi
    sleep 1
  done
  if [ "$ok" != 1 ]; then
    log "scenario '$name': sshd did not show a banner within 30 s"
    docker logs "$cname" >&2 2>&1
    exit 1
  fi
done

# --- tests -------------------------------------------------------------------
export SSHD_KEYS_DIR="$keys_dir"
for k in "${!exports[@]}"; do export "$k=${exports[$k]}"; done

targets=()
for f in "$repo"/crates/ssh-transport/tests/sshd_*.rs; do
  [ -f "$f" ] && targets+=(--test "$(basename "$f" .rs)")
done
[ "${#targets[@]}" -gt 0 ] || { log "no tests/sshd_*.rs target found"; exit 1; }

cd "$repo" || exit 1
cargo test -p ssh-transport "${targets[@]}" -- --ignored --test-threads=1
rc=$?
if [ "$rc" -ne 0 ]; then
  for name in "${scenario_names[@]}"; do
    echo "===== daemon log: $name =====" >&2
    docker logs "$prefix-$name" >&2 2>&1
  done
fi
exit "$rc"
