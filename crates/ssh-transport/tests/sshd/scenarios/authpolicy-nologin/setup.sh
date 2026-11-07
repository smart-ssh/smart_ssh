#!/bin/sh
set -eu
adduser -D -s /sbin/nologin nologinuser
echo "nologinuser:testpass" | chpasswd
