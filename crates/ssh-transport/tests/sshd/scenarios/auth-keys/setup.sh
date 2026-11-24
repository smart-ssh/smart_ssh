# Runs as root before the daemon: authorize the per-run test keys.
set -eu
home=/home/testuser
mkdir -p "$home/.ssh"
cat /keys/user_ed25519.pub /keys/user_rsa.pub > "$home/.ssh/authorized_keys"
chown -R testuser:testuser "$home/.ssh"
chmod 700 "$home/.ssh"
chmod 600 "$home/.ssh/authorized_keys"
