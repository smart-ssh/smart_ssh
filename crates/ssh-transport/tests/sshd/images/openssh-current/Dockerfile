# Throw-away OpenSSH server (as packaged by the pinned alpine) for the
# end-to-end suite (tests/sshd/README.md). User and password are deliberately
# trivial throw-away values that only exist inside the container.
FROM alpine:3.20@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc
RUN apk add --no-cache openssh openssh-sftp-server \
 && adduser -D -s /bin/sh testuser && echo 'testuser:testpass' | chpasswd \
 && printf 'PasswordAuthentication yes\nKbdInteractiveAuthentication no\nAllowTcpForwarding yes\nSubsystem sftp internal-sftp\nPermitRootLogin no\n' > /etc/ssh/sshd_config
COPY entrypoint.sh /entrypoint.sh
EXPOSE 22
ENTRYPOINT ["/bin/sh", "/entrypoint.sh"]
