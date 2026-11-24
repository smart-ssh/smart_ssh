# SSH end-to-end suite (real sshd in Docker)

Runs the SSH transport against real servers: current OpenSSH, OpenSSH 7.4p1 and
Dropbear. All tests are `#[ignore]`, so `cargo test --workspace` never needs
Docker.

## Run locally

Needs Docker, `ssh-keygen` and a Rust toolchain. From the repo root:

```bash
crates/ssh-transport/tests/sshd/run.sh
```

The runner builds the images, generates keys, a CA and a certificate into a
temporary directory (mounted read-only at `/keys`, never committed), starts every
scenario, waits up to 30 s for each banner, runs every `tests/sshd_*.rs` target
with `-- --ignored --test-threads=1`, prints all daemon logs if a test fails and
removes containers and networks on exit. The account in every image is
`testuser` / `testpass` (throw-away).

## Layout

- `images/<image>/Dockerfile` + `entrypoint.sh`: `openssh-current`,
  `openssh-legacy` (7.4p1 built from SHA-256-verified tarballs; each is fetched with retries and from a second source, and a download failure is reported as such by `run.sh`),
  `dropbear`.
- `scenarios/<name>/scenario.toml`: `image`, `hostname`, `networks`
  (list; the hostname is a DNS alias on each), `publish` (bool; maps the port to
  127.0.0.1). Optional: `sshd_config` (replaces the image default),
  `dropbear_flags`, `setup.sh` (runs as root before the daemon; `/keys` and
  `/scenario` are mounted).
- `../sshd_support/mod.rs`: connect helpers, in-memory host key and credential
  stores, `daemon_log`, `pause`/`unpause`/`restart`.

The runner exports `SSHD_<NAME>_PORT` (published scenarios),
`SSHD_<NAME>_CONTAINER` and `SSHD_KEYS_DIR`; `<NAME>` is the scenario name in
upper case with `-` as `_`.

## Add a scenario and a test

1. Create `scenarios/<name>/scenario.toml` (and optional files). No other file
   needs to change.
2. Create `tests/sshd_<area>.rs` with `mod sshd_support;` and `#[ignore]` tests
   that use `sshd_support::connect_scenario("<name>")` etc. Each file uses its
   own scenarios; tests must not depend on the order between files.

CI: `.github/workflows/ssh-e2e.yml` (non-blocking for now).
