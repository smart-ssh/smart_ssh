#!/bin/sh
# First start: only the Ed25519 host key. Every later start (container
# restart, filesystem is kept): only the RSA host key.
if [ -f /etc/ssh/.switched ]; then
  echo 'HostKey /etc/ssh/ssh_host_rsa_key' > /etc/ssh/hostkey-switch.conf
else
  echo 'HostKey /etc/ssh/ssh_host_ed25519_key' > /etc/ssh/hostkey-switch.conf
  touch /etc/ssh/.switched
fi
