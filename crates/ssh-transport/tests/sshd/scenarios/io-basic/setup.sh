# Runs as root before the daemon: fixtures for the listing, permission and
# full-disk tests (no extra privileges needed: /dev/full answers every write
# with ENOSPC).
set -eu
home=/home/testuser
mkdir -p "$home/ls/dir with space"
echo plain > "$home/ls/plain.txt"
echo uni > "$home/ls/ünï cödé 日本.txt"
echo spaced > "$home/ls/file with space.txt"
ln -s plain.txt "$home/ls/link-file"
ln -s "dir with space" "$home/ls/link-dir"
ln -s does-not-exist "$home/ls/dangling"
chown -R testuser:testuser "$home/ls"
# Directory the user cannot write to.
mkdir -p "$home/nowrite"
chmod 755 "$home/nowrite"
# Upload target that is always full.
ln -s /dev/full "$home/full"
