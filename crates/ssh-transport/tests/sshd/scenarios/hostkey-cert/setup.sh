#!/bin/sh
# Signs the Ed25519 host key with the per-run test CA (mounted at /keys).
set -eu
ssh-keygen -q -s /keys/ca -I hostkey-cert -h -n 127.0.0.1,localhost -V -5m:+1d /etc/ssh/ssh_host_ed25519_key.pub
