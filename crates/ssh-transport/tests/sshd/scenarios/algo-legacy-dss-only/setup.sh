# Installs the throw-away RSA user key (generated per run by the runner).
install -d -m 700 -o testuser -g testuser /home/testuser/.ssh
install -m 600 -o testuser -g testuser /keys/user_rsa.pub /home/testuser/.ssh/authorized_keys
ssh-keygen -q -t dsa -N "" -f /etc/ssh/ssh_host_dsa_key
