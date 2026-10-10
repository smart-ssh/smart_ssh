#!/usr/bin/env bash
# Startet Bastion und Ziel als OpenSSH-Container in einem gemeinsamen
# Docker-Netz und führt die ignorierten Tests aus `tests/openssh_jump.rs`
# aus. Aufruf vom Repo-Root: crates/ssh-transport/tests/openssh/run-jump-tests.sh
# Braucht Docker und eine Rust-Toolchain; Ports per SSH_JUMP_*-Variablen.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
image="smart-ssh-openssh-jump-test"
net="smart-ssh-jump-net"
bastion_port="${SSH_JUMP_BASTION_PORT:-2301}"
target_port="${SSH_JUMP_TARGET_PORT:-2302}"

cleanup() {
  docker rm -f jump-bastion jump-target >/dev/null 2>&1 || true
  docker network rm "$net" >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup

docker build -t "$image" "$here"
docker network create "$net" >/dev/null
docker run -d --name jump-bastion --hostname jump-bastion --network "$net" \
  -p "127.0.0.1:${bastion_port}:22" "$image" >/dev/null
docker run -d --name jump-target --hostname jump-target --network "$net" \
  -p "127.0.0.1:${target_port}:22" "$image" >/dev/null

# Warten, bis beide sshd einen Banner liefern (max. 30 s, sonst Fehler).
for port in "$bastion_port" "$target_port"; do
  ok=0
  for _ in $(seq 1 30); do
    if timeout 2 bash -c "exec 3<>/dev/tcp/127.0.0.1/$port && head -c 4 <&3" 2>/dev/null | grep -q SSH-; then
      ok=1; break
    fi
    sleep 1
  done
  [ "$ok" = 1 ] || { echo "sshd auf Port $port nicht erreichbar" >&2; exit 1; }
done

cd "$repo"
SSH_JUMP_BASTION_PORT="$bastion_port" SSH_JUMP_TARGET_PORT="$target_port" \
SSH_JUMP_TARGET_HOST=jump-target \
  cargo test -p ssh-transport --test openssh_jump -- --ignored --test-threads=1
