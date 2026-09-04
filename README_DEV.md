# Developer Guide & Release Management

This document describes how versioning, builds, releases, and CI/CD workflows are managed in this repository.

---

## 1. Version Management

The application version is maintained in **three synchronized locations**:

| Component | File | JSON / TOML Path |
| :--- | :--- | :--- |
| **Rust Workspace & Crates** | [`Cargo.toml`](./Cargo.toml) | `[workspace.package] -> version = "0.1.0"` |
| **Tauri Desktop App** | [`crates/app-tauri/tauri.conf.json`](./crates/app-tauri/tauri.conf.json) | `"version": "0.1.0"` |
| **Frontend Web App** | [`crates/app-tauri/frontend/package.json`](./crates/app-tauri/frontend/package.json) | `"version": "0.1.0"` |

All individual Rust crates in `crates/*` automatically inherit their version from `[workspace.package]` via `version.workspace = true`.

---

## 2. Version Bump & Release Process

Follow these steps to bump the version and trigger automated multi-platform release builds:

### Step 1: Update Version Numbers
Update the version string (e.g. from `0.1.0` to `0.2.0`) in all three files:
1. `Cargo.toml`
2. `crates/app-tauri/tauri.conf.json`
3. `crates/app-tauri/frontend/package.json`

### Step 2: Commit Changes
```bash
git add Cargo.toml crates/app-tauri/tauri.conf.json crates/app-tauri/frontend/package.json
git commit -m "chore: bump version to 0.2.0"
```

### Step 3: Create and Push Git Tag
Tag the commit using the format `v<version>`:
```bash
git tag v0.2.0
git push origin main --tags
```

---

## 3. Automated CI/CD & Executable Builds

### Release Workflow ([`.github/workflows/release.yml`](./.github/workflows/release.yml))
When a `v*` tag is pushed (or triggered manually via **Workflow Dispatch** in GitHub Actions):
- Builds native bundles across:
  - **macOS** (`macos-latest`): Universal binary (`.dmg`, `.app` for both Apple Silicon and Intel)
  - **Windows** (`windows-latest`): `.msi` and `.exe` installers
  - **Linux** (`ubuntu-22.04`): `.deb` and `.AppImage` packages
- Automatically creates a GitHub Draft Release under the **Releases** section with all installers and binaries attached for download.

### CI Workflow ([`.github/workflows/ci.yml`](./.github/workflows/ci.yml))
Runs on every pull request and branch push:
- Code formatting (`cargo fmt --all --check`)
- Lints (`cargo clippy --workspace --all-targets -- -D warnings`)
- Workspace unit & integration tests (`cargo test --workspace`)

---

## 4. Building Executables Locally

To produce release executables and installer bundles locally:

### Prerequisites
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- [Node.js](https://nodejs.org/) (>= 20)
- On Linux only: WebKit2GTK and related build packages (`libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev`)

### Build Commands
```bash
# 1. Install frontend dependencies
cd crates/app-tauri/frontend
npm install

# 2. Build the Tauri desktop application and bundles
npx tauri build
```

### Artifact Output Locations
Once built, packages will be located in the root `target/release/` folder:

| Platform | Format | Output Directory |
| :--- | :--- | :--- |
| **macOS** | `.dmg` / `.app` | `target/release/bundle/dmg/` & `target/release/bundle/macos/` |
| **Windows** | `.msi` / `.exe` | `target/release/bundle/msi/` & `target/release/bundle/nsis/` |
| **Linux** | `.deb` / `.AppImage` | `target/release/bundle/deb/` & `target/release/bundle/appimage/` |
| **Raw Binary** | Executable binary | `target/release/smart-ssh` |

---

## 5. Local Development (`cargo tauri dev`) — Stable macOS Code Signature

**Use `./scripts/tauri-dev.sh` instead of `cargo tauri dev` directly on
macOS.** This app stores secrets (SSH credentials, sudo password, AI
provider API keys) in the OS keychain (Spec 0003). On macOS, a "Always
Allow" grant for a keychain item is bound to the accessing app's code
signature. Plain `cargo tauri dev` runs the raw binary with only the
Rust linker's automatic ad-hoc signature, which is content-hash-based and
therefore **changes on every rebuild** — macOS treats each rebuild as a
new, untrusted app and discards previously granted keychain permissions,
causing a fresh permission prompt after almost every code change.

`./scripts/tauri-dev.sh`:
1. Runs `scripts/setup-macos-dev-signing.sh` once (idempotent) to create
   and trust a self-signed, project-specific code-signing certificate
   ("Smart SSH Dev Signing") in your login keychain — no manual Keychain
   Access steps needed.
2. Runs `cargo tauri dev` with a custom `--runner`
   (`scripts/tauri-dev-stable-signing-runner.sh`) that re-signs the built
   binary with that stable certificate before launching it, on every
   rebuild the file-watcher triggers.

This is **deliberate, load-bearing configuration** — do not remove it as
"unnecessary complexity". It exists specifically because
`bundle.macOS.signingIdentity` / `APPLE_SIGNING_IDENTITY` (the commonly
documented Tauri fix) only affects `tauri build`'s bundler signing step,
not `tauri dev`, which never invokes the bundler on macOS at all — see
[`docs/adr/0022-stable-dev-code-signature.md`](docs/adr/0022-stable-dev-code-signature.md)
for the full investigation and empirical verification. On Windows/Linux,
`tauri-dev.sh` just runs plain `cargo tauri dev` (this problem is
macOS-specific).

**How to verify it's working**: run `./scripts/tauri-dev.sh`, make a
source change (triggering a rebuild), and compare
`codesign -dv --verbose=4 target/debug/smart-ssh` before and
after — `CDHash` will differ (expected, content changed), but
`codesign -d -r- target/debug/smart-ssh` should show the same
designated requirement (`identifier "smart-ssh" and
certificate leaf = H"..."`) both times. A keychain permission you granted
before the change should then still be honored after it, without a new
prompt.

---

## 6. macOS Release Signing & Notarization (Official Edition)

Release builds distributed as the Official Edition are signed with a
Developer ID Application certificate and notarized by Apple — separate
from, and unrelated to, the dev-signing setup in section 5 above (that
one only stabilizes `cargo tauri dev`'s ad-hoc signature; it never
touches `bundle.macOS` in `tauri.conf.json`, so the two coexist without
interference, see
[`docs/adr/0022-stable-dev-code-signature.md`](docs/adr/0022-stable-dev-code-signature.md)).

**This applies only to the person building the Official Edition with
their own Apple Developer account credentials — a plain `cargo tauri
build` from a fresh clone, with none of the environment variables below
set, still produces an unsigned Community-Edition-equivalent bundle. The
checked-in `tauri.conf.json` deliberately does not hardcode a
`signingIdentity` (that value is specific to one developer's
certificate) — it comes exclusively from the `APPLE_SIGNING_IDENTITY`
environment variable below, which `tauri-cli` always prefers over any
config value when set.**

### Required environment variables

None of these are checked into the repository, and the `.env` file
holding their values must never be committed. Names only, no values —
the actual file lives outside the repo at `~/build/smart-ssh-signing.env`:

| Variable | Purpose |
| :--- | :--- |
| `APPLE_SIGNING_IDENTITY` | The Developer ID Application certificate's SHA-1 fingerprint (not the display name — see `docs/adr/0033-app-store-connect-api-key-notarization.md` for why a fingerprint, not a name). |
| `APPLE_API_KEY` | App Store Connect API Key ID. |
| `APPLE_API_ISSUER` | App Store Connect API Issuer ID (UUID). |
| `APPLE_API_KEY_PATH` | Local filesystem path to the downloaded `AuthKey_<APPLE_API_KEY>.p8` private key file. |

### Build flow

```bash
source ~/build/smart-ssh-signing.env
cd crates/app-tauri/frontend && npm install && cd ..
cargo tauri build
```

(`cargo tauri build` must run from `crates/app-tauri` — that's where
`tauri.conf.json` and the `Cargo.toml` `tauri` dependency live, same as
`./scripts/tauri-dev.sh`/`cargo tauri dev` in section 5.)

`cargo tauri build` builds the frontend, compiles the release binary,
bundles the `.app`/`.dmg`, signs it with the Developer ID identity from
`APPLE_SIGNING_IDENTITY` (entitlements from
[`crates/app-tauri/entitlements.plist`](./crates/app-tauri/entitlements.plist),
Hardened Runtime enabled — both configured in `tauri.conf.json`'s
`bundle.macOS`), then submits it to Apple's notarization service using
the App Store Connect API key credentials and staples the resulting
ticket onto the bundle.

**Notarization is the slow part** — Apple's service typically takes a
few minutes, but can occasionally take up to about an hour under load.
`cargo tauri build` blocks and polls until it completes (or the
credentials are rejected/missing, which fails fast instead). Don't
interrupt it while it's waiting; there's no separate "check notarization
status later" step to resume with the current tooling.

### Artifacts

Same locations as section 4 above
(`target/release/bundle/macos/`, `target/release/bundle/dmg/`) — now
signed and notarized instead of unsigned. Verify with:

```bash
codesign -dv --verbose=4 "target/release/bundle/macos/Smart SSH.app"
spctl -a -vv "target/release/bundle/macos/Smart SSH.app"
```

**Never commit `~/build/smart-ssh-signing.env`, the `.p8` API key file,
or any of the values above.** They belong exclusively on the machine(s)
used to produce Official Edition releases.
