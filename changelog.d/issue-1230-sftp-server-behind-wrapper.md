### Fixed
- A command that starts `sftp-server` behind a wrapper such as `timeout 5m` or `nice -n 10` is flagged and asks for confirmation again, also when part of it is quoted.
