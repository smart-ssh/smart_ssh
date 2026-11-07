#!/bin/sh
set -eu
# keyboard-interactive needs the PAM-enabled sshd, which alpine ships as
# /usr/sbin/sshd.pam; the entrypoint starts /usr/sbin/sshd, so swap it in.
apk add --no-cache openssh-server-pam linux-pam >/dev/null
cp -f /usr/sbin/sshd.pam /usr/sbin/sshd
# Two prompt rounds: pam_unix does not hand its password on (not_set_pass),
# so the second pam_unix asks for it again. The stored password answers
# the first round, the second one can only come from the prompter.
cat > /etc/pam.d/sshd <<PAM
auth required pam_unix.so not_set_pass
auth required pam_unix.so
account required pam_permit.so
session required pam_permit.so
PAM
