### Security
- Reading an SSH private key through a wildcard (`cat ~/.ssh/id_*`, `cat ~/.ssh/backup_id_*`) is now rated red data risk. Public keys and `known_hosts` are not affected.
