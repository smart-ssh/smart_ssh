# Local patch of russh 0.63.2 (issue #1228)

Only change versus the crates.io release (`src/client/encrypted.rs`,
`USERAUTH_BANNER` branch): the banner is read as raw bytes and converted with
`String::from_utf8_lossy`, instead of a strict `String::decode` that fails the
login on invalid UTF-8. Remove this directory and the `[patch.crates-io]`
section in the root `Cargo.toml` once a russh release contains the fix.
Upstream licence: Apache-2.0 (see `Cargo.toml`).

```diff
-let banner = map_err!(String::decode(&mut r))?;
+let banner = String::from_utf8_lossy(&map_err!(Vec::<u8>::decode(&mut r))?).into_owned();
```
