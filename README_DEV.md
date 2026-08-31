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
| **Raw Binary** | Executable binary | `target/release/ssh-manager-app-tauri` |
