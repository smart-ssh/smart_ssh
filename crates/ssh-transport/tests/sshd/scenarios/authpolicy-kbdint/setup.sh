#!/bin/sh
set -eu
# keyboard-interactive needs the PAM-enabled sshd, which alpine ships as
# /usr/sbin/sshd.pam; the entrypoint starts /usr/sbin/sshd, so swap it in.
apk add --no-cache openssh-server-pam linux-pam >/dev/null
cp -f /usr/sbin/sshd.pam /usr/sbin/sshd
cat > /etc/pam.d/sshd <<EOF
auth required pam_unix.so
account required pam_permit.so
session required pam_permit.so
EOF
