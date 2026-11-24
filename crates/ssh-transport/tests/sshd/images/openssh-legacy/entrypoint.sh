#!/bin/sh
# Host keys are generated per container start; the scenario may bring its own
# sshd_config (replaces the default) and a setup.sh (runs as root first).
set -eu
ssh-keygen -A >/dev/null
if [ -f /scenario/setup.sh ]; then sh /scenario/setup.sh; fi
cfg=/etc/ssh/sshd_config
if [ -f /scenario/sshd_config ]; then cfg=/scenario/sshd_config; fi
exec /usr/local/sbin/sshd -D -e -f "$cfg"
