#!/usr/bin/with-contenv bash
# Test-Infrastruktur, kein Sicherheitsanspruch: erlaubt testuser
# passwortloses sudo, damit die KI vorgeschlagene sudo-Kommandos über den
# reinen Exec-Channel (kein TTY, kein Passwort-Prompt möglich) tatsächlich
# ausführen kann.
echo "testuser ALL=(ALL) NOPASSWD: ALL" > /etc/sudoers.d/testuser-nopasswd
chmod 0440 /etc/sudoers.d/testuser-nopasswd
