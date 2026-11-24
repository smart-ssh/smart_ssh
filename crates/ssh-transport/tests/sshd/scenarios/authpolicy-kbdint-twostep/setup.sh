#!/bin/sh
set -eu
# keyboard-interactive needs the PAM-enabled sshd, which alpine ships as
# /usr/sbin/sshd.pam; the entrypoint starts /usr/sbin/sshd, so swap it in.
apk add --no-cache openssh-server-pam linux-pam >/dev/null
cp -f /usr/sbin/sshd.pam /usr/sbin/sshd
# Two prompt rounds: after pam_unix took the password, pam_ftp asks a visible-input prompt (an e-mail
# address) for the listed user; the stored password cannot answer an echoed
# prompt, so the prompter must.
cat > /etc/pam.d/sshd <<PAM
auth required pam_unix.so
auth required pam_ftp.so ignore users=testuser
account required pam_permit.so
session required pam_permit.so
PAM
