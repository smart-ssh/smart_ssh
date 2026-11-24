# Runs as root before the daemon: the alpine dropbear package ships no SFTP
# server; install OpenSSH's and make it findable at dropbear's default path.
set -eu
apk add --no-cache openssh-sftp-server >/dev/null
mkdir -p /usr/libexec
ln -sf /usr/lib/ssh/sftp-server /usr/libexec/sftp-server
