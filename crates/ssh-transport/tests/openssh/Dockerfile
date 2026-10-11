# Wegwerf-OpenSSH-Server für die Jump-Host-Ende-zu-Ende-Tests
# (`tests/openssh_jump.rs`, Issue #180, ADR 0008 Update). Nur für Tests:
# Benutzer und Passwort sind absichtlich triviale Wegwerfwerte, die nur
# innerhalb des Containers gelten.
FROM alpine:3.20
RUN apk add --no-cache openssh openssh-sftp-server \
 && adduser -D -s /bin/sh testuser && echo 'testuser:testpass' | chpasswd \
 && ssh-keygen -A \
 && printf 'PasswordAuthentication yes\nKbdInteractiveAuthentication no\nAllowTcpForwarding yes\nSubsystem sftp internal-sftp\nPermitRootLogin no\n' > /etc/ssh/sshd_config
EXPOSE 22
CMD ["/usr/sbin/sshd","-D","-e"]
