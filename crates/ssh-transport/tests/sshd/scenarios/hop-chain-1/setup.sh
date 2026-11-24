#!/bin/sh
# Installs the per-run test public key for testuser (key at /keys, never committed).
set -eu
install -d -m 700 -o testuser -g testuser /home/testuser/.ssh
install -m 600 -o testuser -g testuser /keys/user_ed25519.pub /home/testuser/.ssh/authorized_keys
