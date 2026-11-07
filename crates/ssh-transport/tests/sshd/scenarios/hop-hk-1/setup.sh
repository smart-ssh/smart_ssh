#!/bin/sh
# New host keys on every start (a restart simulates a re-installed server).
rm -f /etc/ssh/ssh_host_*
ssh-keygen -A >/dev/null
