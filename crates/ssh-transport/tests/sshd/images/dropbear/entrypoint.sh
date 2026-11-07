#!/bin/sh
# The scenario may replace the default flags with a `dropbear_flags` file
# (one line, e.g. "-F -E -R -p 22") and bring a setup.sh (runs as root first).
set -eu
if [ -f /scenario/setup.sh ]; then sh /scenario/setup.sh; fi
flags="-F -E -R -p 22"
if [ -f /scenario/dropbear_flags ]; then flags="$(cat /scenario/dropbear_flags)"; fi
# shellcheck disable=SC2086
exec /usr/sbin/dropbear $flags
