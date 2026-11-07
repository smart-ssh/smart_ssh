#!/bin/sh
set -eu
# keyboard-interactive needs the PAM-enabled sshd, which alpine ships as
# /usr/sbin/sshd.pam; the entrypoint starts /usr/sbin/sshd, so swap it in.
apk add --no-cache openssh-server-pam linux-pam >/dev/null
cp -f /usr/sbin/sshd.pam /usr/sbin/sshd
# Second factor: a fixed test code, asked as a second password-style prompt.
# pam_unix does not hand its password on (not_set_pass), so pam_exec
# prompts again.
cat > /usr/local/bin/pam-code <<'SCRIPT'
#!/bin/sh
code=""
read -r code || true
[ "$code" = "424242" ]
SCRIPT
chmod 755 /usr/local/bin/pam-code
cat > /etc/pam.d/sshd <<PAM
auth required pam_unix.so not_set_pass
auth required pam_exec.so expose_authtok quiet /usr/local/bin/pam-code
account required pam_permit.so
session required pam_permit.so
PAM
