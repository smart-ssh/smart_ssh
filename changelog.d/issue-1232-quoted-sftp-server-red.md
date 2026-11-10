### Fixed
- A command that starts `sftp-server` behind wrappers and quoted arguments (e.g. `timeout 5m env FOO='a b' sftp-server`) is now rated red like the plain form.
