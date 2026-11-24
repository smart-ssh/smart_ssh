### Security
- A command that starts `sftp-server` through `sudo`/`doas` is now always rated red and asks for confirmation, also when a sudo option has a quoted argument such as `-p '[sudo] password:'`.
