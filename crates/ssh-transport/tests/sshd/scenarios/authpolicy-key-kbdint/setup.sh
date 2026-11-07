#!/bin/sh
set -eu
# keyboard-interactive needs the PAM-enabled sshd; this scenario installs it at start.
apk add --no-cache openssh-server-pam linux-pam >/dev/null 2>&1 || {
  apk del openssh openssh-server >/dev/null 2>&1
  apk add --no-cache openssh-server-pam linux-pam >/dev/null
}
cat > /etc/pam.d/sshd <<EOF
auth required pam_unix.so
account required pam_permit.so
session required pam_permit.so
EOF
# Authorize the per-run test key for a user.
install_key() {
  home="$(getent passwd "$1" | cut -d: -f6)"
  mkdir -p "$home/.ssh"
  cp /keys/user_ed25519.pub "$home/.ssh/authorized_keys"
  chown -R "$1:$1" "$home/.ssh"
  chmod 700 "$home/.ssh"
  chmod 600 "$home/.ssh/authorized_keys"
}
install_key testuser
