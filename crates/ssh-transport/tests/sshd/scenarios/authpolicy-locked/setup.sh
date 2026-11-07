#!/bin/sh
set -eu
# Authorize the per-run test key for a user.
install_key() {
  home="$(getent passwd "$1" | cut -d: -f6)"
  mkdir -p "$home/.ssh"
  cp /keys/user_ed25519.pub "$home/.ssh/authorized_keys"
  chown -R "$1:$1" "$home/.ssh"
  chmod 700 "$home/.ssh"
  chmod 600 "$home/.ssh/authorized_keys"
}
adduser -D -s /bin/sh lockeduser
install_key lockeduser
# Lock the account after the key is in place.
passwd -l lockeduser
