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
