# OpenSSH 7.4p1 (2016) built from the release tarball against OpenSSL 1.0.2u,
# the last OpenSSL line this OpenSSH release supports. Both tarballs are
# verified by SHA-256. The default config is modern-compatible; scenarios
# enable legacy algorithms (ssh-rsa, diffie-hellman-group1-sha1, CBC ciphers,
# hmac-sha1) in their own sshd_config.
FROM alpine:3.20@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc AS build
RUN apk add --no-cache build-base perl linux-headers zlib-dev curl
# alpine's fortify wrappers clash with the realpath() compat shim of OpenSSH 7.4
RUN rm -rf /usr/include/fortify
WORKDIR /src
RUN curl -fsSLO https://www.openssl.org/source/old/1.0.2/openssl-1.0.2u.tar.gz \
 && echo 'ecd0c6ffb493dd06707d38b14bb4d8c2288bb7033735606569d8f90f89669d16  openssl-1.0.2u.tar.gz' | sha256sum -c - \
 && tar xzf openssl-1.0.2u.tar.gz && cd openssl-1.0.2u \
 && ./config --prefix=/opt/openssl --openssldir=/opt/openssl no-shared no-asm -fPIC \
 && make -j"$(nproc)" && make install_sw
RUN curl -fsSLO https://cdn.openbsd.org/pub/OpenBSD/OpenSSH/portable/openssh-7.4p1.tar.gz \
 && echo '1b1fc4a14e2024293181924ed24872e6f2e06293f3e8926a376b8aec481f19d1  openssh-7.4p1.tar.gz' | sha256sum -c - \
 && tar xzf openssh-7.4p1.tar.gz && cd openssh-7.4p1 \
 && ./configure --prefix=/usr/local --sysconfdir=/etc/ssh --with-ssl-dir=/opt/openssl \
      --with-privsep-path=/var/empty --without-pam \
 && make -j"$(nproc)" && make DESTDIR=/out install-nokeys

FROM alpine:3.20@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc
RUN apk add --no-cache zlib \
 && adduser -D -s /bin/sh testuser && echo 'testuser:testpass' | chpasswd \
 && mkdir -p /var/empty /etc/ssh \
 && printf 'PasswordAuthentication yes\nChallengeResponseAuthentication no\nAllowTcpForwarding yes\nSubsystem sftp internal-sftp\nPermitRootLogin no\n' > /etc/ssh/sshd_config
COPY --from=build /out/usr/local /usr/local
COPY --from=build /out/etc/ssh/moduli /etc/ssh/moduli
COPY entrypoint.sh /entrypoint.sh
EXPOSE 22
ENTRYPOINT ["/bin/sh", "/entrypoint.sh"]
