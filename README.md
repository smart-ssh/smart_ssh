# Smart SSH

[![CI](https://github.com/smart-ssh/smart_ssh/actions/workflows/community.yml/badge.svg)](https://github.com/smart-ssh/smart_ssh/actions/workflows/community.yml)
[![License: GPL-3.0-only](https://img.shields.io/badge/License-GPL--3.0--only-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.99%2B-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-24C8D8.svg)](https://tauri.app/)

**Smart SSH** is a desktop SSH client with a server manager and an AI assistant whose every proposed command passes a local policy engine (allow / confirm / deny) — the AI never sees your keys or passwords.

## Why

- **Every command is checked locally.** The policy engine decides allow, confirm or deny, explains which rule matched, and keeps a hard blacklist that no rule can override.
- **No tricks with chaining or substitution.** Operators (`&&`, `||`, `;`, `|`) and command substitutions (`$(...)`, backticks) are split apart and each part is checked.
- **Secrets stay local.** Output is redacted before anything is sent to the AI provider, and credentials live in the OS keychain or in an encrypted database.
- **Bring your own AI.** Use your own OpenAI, Anthropic or OpenRouter key, or a local model through Ollama.
- **Coding agents included.** An optional MCP endpoint lets tools such as Claude Code propose commands through the same policy engine.

## Install

Installers are on the download section of the project website: <https://smart-ssh.dev/#download>.

- **macOS:** Apple Silicon and Intel.
- **Windows:** x64.
- **Linux:** `.deb`, `.rpm` or `.AppImage`.

On Linux, Smart SSH needs a Secret Service (for example gnome-keyring) to keep its database key, or you set a master password instead. See [Data paths](docs/datenpfade.md) for where Smart SSH stores its data on each platform.

## Five-minute path

1. Click **+ Server** (or **Add first server**), enter host, port and username, and choose how to authenticate.
2. Click **Test Connection** to check host, port and credentials before you save.
3. Click **Connect** and, on first contact, compare the fingerprint in the **Unknown Host** dialog and click **Trust**.
4. Open **Settings** → **AI Provider** and add a provider: **Use Ollama** for a local model, or **Add Provider** with your own OpenAI or Anthropic key, or OpenRouter through the generic OpenAI-compatible type.
5. Ask in plain language in the chat, for example "which service uses the most memory?".
6. Review the proposed command in the confirmation dialog, which explains which rule matched and lets you edit the command.
7. Click **Execute** (or **Deny**) and read the result in the chat.
8. Close the app; when you connect to the server again, pick the conversation under **Past conversations** to continue it.

## Use with Claude Code and other MCP clients

1. Open **Settings** → **MCP Server** and enable **Enable MCP server**.
2. In **Server allow list (reachable via MCP)**, tick the servers the client may use.
3. Copy the configuration with **Copy configuration**, or use this block (replace `<token>` with the token shown in the settings):

```json
{
  "mcpServers": {
    "smart-ssh": {
      "type": "http",
      "url": "http://127.0.0.1:47823/mcp",
      "headers": {
        "Authorization": "Bearer <token>"
      }
    }
  }
}
```

The equivalent Claude Code command:

```bash
claude mcp add --transport http smart-ssh http://127.0.0.1:47823/mcp --header "Authorization: Bearer <token>"
```

MCP is off by default, and only servers on the allow list are reachable. Every command a client proposes passes the same policy engine as your own and appears in the confirmation dialog in its own tab. Agents get no extra auto-execution: a command only runs after you confirm it in the app.

## Security

- [Threat model](THREAT-MODEL.md) — what the AI sees and never sees, limits of redaction, data storage, network connections and known residual risks.
- [SECURITY.md](SECURITY.md) — how to report a vulnerability.
- [KNOWN-ISSUES.md](KNOWN-ISSUES.md) — current limitations and deliberate design decisions.

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the spec-first workflow, the local checks and how pull requests are reviewed. Contributions need a one-time signature of the [CLA](CLA.md) in your first pull request.

## Build from source

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.99+ (the exact version is pinned in `rust-toolchain.toml`)
- [Node.js](https://nodejs.org/) 20+ and `npm`
- Platform-specific Tauri dependencies (see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/))

### Run in development mode

```bash
# 1. Clone the repository
git clone https://github.com/smart-ssh/smart_ssh.git
cd smart_ssh

# 2. Install the frontend dependencies
cd apps/smart-ssh-community/frontend
npm install
cd ../../..

# 3. Start the desktop app in dev mode
cargo tauri dev --manifest-path apps/smart-ssh-community/Cargo.toml
# or from apps/smart-ssh-community (where tauri.conf.json lives):
cd apps/smart-ssh-community
./frontend/node_modules/.bin/tauri dev
```

### Tests and linting

```bash
# Run all unit and integration tests in the workspace
cargo test --workspace

# Check code formatting
cargo fmt --all --check

# Check Clippy lints
cargo clippy --workspace --all-targets -- -D warnings
```

More in the [developer and release guide](README_DEV.md).

## Translations

German and English are maintained by hand. All other languages are machine-translated and may contain mistakes. Please report wrong or unsuitable translations as an issue (language, place in the app, better text) — corrections are welcome. Terminology: [docs/i18n-glossary.md](docs/i18n-glossary.md).

## Documentation

- [Threat model](THREAT-MODEL.md) — what the AI sees and never sees, limits of redaction, data storage and encryption, network connections, confirmation boundaries and known residual risks
- [Data paths](docs/datenpfade.md) — where Smart SSH stores the database, logs, host keys and settings on each platform
- [Architecture](docs/architecture.md) — crate layout and boundaries
- [Developer and release guide](README_DEV.md) — versioning, release workflow and GitHub secrets
- [Feature specifications](docs/specs/INDEX.md) — all specs with status and purpose
- [Architecture decision records](docs/adr/INDEX.md) — all architecture decisions
- [Changelog](CHANGELOG.md)

## License

Smart SSH is licensed under the **GNU General Public License v3.0** (GPL-3.0-only) from version 0.8.0 — see [LICENSE](LICENSE). Versions up to and including 0.7.0 remain under the Apache License 2.0. Copyright notice: [NOTICE](NOTICE).
