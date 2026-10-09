# Smart SSH — Threat model

This document states, as plainly as we can, what the AI copilot in Smart SSH
can and cannot see, where your data lives, which network connections the app
makes, which actions always need your confirmation, and which risks remain.
Every statement links the spec, ADR or code that backs it. Specs and ADRs are
written in German; file and section references are given so you can check
each claim.

If a statement here disagrees with the code, the code is wrong or this
document is wrong — please open an issue.

**Scope.** The Community edition in this repository: the desktop app, its
local database, its connections to SSH servers and to the AI providers you
configure, and the optional local MCP server. Out of scope: the security of
your SSH servers themselves, of the AI provider you choose, and of the
operating system account Smart SSH runs under. Anyone who controls your OS
account (or can read process memory while the app is unlocked) is outside
what this app can defend against.

---

## 1. What the AI sees — and what it never sees

### What is sent to the AI provider

When you chat in a session, the active AI provider receives:

- **A system prompt** with the server's display name, the allow rules that
  apply to this server, and the server and group notes. Hostname, user name
  and tags of the profile are not part of the prompt.
  ([`crates/app-logic/src/system_prompt.rs`](crates/app-logic/src/system_prompt.rs),
  [`crates/app-shell/src/commands/connect.rs`](crates/app-shell/src/commands/connect.rs),
  [`crates/app-logic/src/compaction.rs`](crates/app-logic/src/compaction.rs))
- **The remote OS banner** (`uname -a`) read at connect time, as fenced
  untrusted content.
- **Your chat messages and the chat history** of the session (redacted before
  sending, see section 2).
- **Output of commands that ran** (stdout/stderr) and **contents of remote
  files the AI asked to read** — both redacted, then fenced as untrusted
  content.
- **The commands themselves** that the AI proposed or that you approved.

Content that comes from the server is treated as untrusted data, never as
instructions: it is wrapped in tagged fences with `<`, `>` and `&` escaped,
and the system prompt tells the model not to follow instructions inside them
([Spec 0039](docs/specs/0039-untrusted-content-fencing.md),
[ADR 0027](docs/adr/0027-post-ingest-policy-scope-and-injection-check-timing.md),
[ADR 0034](docs/adr/0034-redact-before-fence-marker-splitting.md),
[`crates/core/src/ai/fencing.rs`](crates/core/src/ai/fencing.rs)).
Fencing makes prompt injection harder; it does not make it impossible —
which is why the confirmation boundaries in section 5 exist.

Besides the chat itself, the app makes further AI calls inside a session you
started, only to providers you configured: history compaction, session
title, note suggestion, note shortening, and — if you enable them — a risk
second opinion and an injection check. The full list with triggers is in
[docs/netzwerkverbindungen.md](docs/netzwerkverbindungen.md). Compaction,
title, note suggestion and note shortening send redacted history; the
injection check receives redacted output. The **risk second opinion receives
the proposed command or path text as is**, without redaction
([`crates/app-logic/src/second_opinion.rs`](crates/app-logic/src/second_opinion.rs)).

### What the AI never sees

- **Credentials from the credential store**: SSH passwords, private keys,
  key passphrases, certificates, stored sudo passwords, AI provider API keys
  and the MCP token are never put into a prompt. The prompt is built from the
  server name, rules, notes and session content only
  ([`crates/app-logic/src/system_prompt.rs`](crates/app-logic/src/system_prompt.rs)).
- **Identity files on disk**: a key file referenced by path is read only to
  authenticate, its buffer is zeroed afterwards
  ([Spec 0076](docs/specs/0076-identity-file-auth.md),
  [`crates/app-logic/src/key_files.rs`](crates/app-logic/src/key_files.rs)).
- **A stored sudo password**: when a command needs it, the command is
  rewritten to `sudo -S …` and the password is written to the command's
  stdin. The command text that reaches the result, log and AI context
  contains `-S` but not the password. The session's redactor additionally
  knows the password as an extra pattern, so if the server echoes it, it is
  masked ([`crates/app-logic/src/orchestration/action_exec.rs`](crates/app-logic/src/orchestration/action_exec.rs),
  [`crates/app-logic/src/server_redaction.rs`](crates/app-logic/src/server_redaction.rs)).

### What the AI can still see

"Never sees credentials from the credential store" does **not** mean "never
sees a secret". The AI sees whatever the server prints and whatever you type
into the chat, minus what the redactor recognises (section 2). Examples:

- a secret in a configuration file the AI reads, if its format is not one the
  redactor knows;
- a password you paste into a chat message;
- a secret you wrote into a server or group note (notes are sent as written,
  fenced but not redacted, because you authored them).

---

## 2. Redaction — and its limits

Before content leaves the app towards an AI provider, it passes through the
output redactor
([`crates/core/src/ai/redactor.rs`](crates/core/src/ai/redactor.rs)).
Redaction also runs before ledger entries and note suggestions are stored,
and before server notes are returned to an MCP client
([ADR 0103](docs/adr/0103-mcp-server-notes-redact-and-fence.md)).

### What is redacted

The redactor recognises a **finite list of known secret formats**:

- private key blocks (`-----BEGIN … PRIVATE KEY-----`);
- `password=`, `token=`, `api_key=`-style assignments;
- well-known token formats: AWS, GitHub, GitLab, Slack, Stripe, Google, npm,
  Anthropic, OpenAI, OpenRouter, Hugging Face, Twilio, SendGrid, JWTs,
  Azure `AccountKey=`;
- `Authorization: Bearer/Basic` and `x-api-key` headers;
- credentials in URLs and database connection strings, including `@` inside
  the password and credentials as query parameters
  ([Spec 0078](docs/specs/0078-redactor-at-in-url-password.md),
  [ADR 0069](docs/adr/0069-redactor-at-in-url-credentials-decisions.md));
- password hashes (crypt/shadow formats, Argon2, phpass);
- passwords on common command lines: `mysql -p…`, `sshpass -p`, `curl -u`,
  `htpasswd -b`, `redis-cli -a`, `smbclient -U`, `openssl -pass`
  ([Spec 0095](docs/specs/0095-redactor-command-line-passwords-and-tokens.md),
  [ADR 0087](docs/adr/0087-command-line-password-redaction-boundaries.md));
- the stored sudo password of the current server, if one exists.

New patterns were added in
[Spec 0068](docs/specs/0068-security-hardening-prelaunch.md),
[Spec 0078](docs/specs/0078-redactor-at-in-url-password.md) and
[Spec 0095](docs/specs/0095-redactor-command-line-passwords-and-tokens.md).

### The explicit limit

> **We redact known secret formats. Unusual formats can slip through. Do not
> connect production servers with unusual secret layouts to external AI
> providers.**

Concretely, the following can reach the AI provider in plaintext:

- proprietary or in-house token formats the redactor has no pattern for;
- connection strings or config lines that carry a secret without a
  recognisable key such as `password=`;
- unusual hash layouts;
- an ad-hoc password that appears on its own, without a recognisable
  context.

A generic "high-entropy string" fallback was considered and **rejected**
([Spec 0068](docs/specs/0068-security-hardening-prelaunch.md);
see the comments in
[`crates/core/src/ai/redactor.rs`](crates/core/src/ai/redactor.rs)): it
would mask commit hashes, checksums, IDs and similar values the copilot
needs to do its job, while still not reliably catching short or low-entropy
secrets. Known edge cases of individual patterns are documented, e.g.
values that stop at `,`, `;` or `"`
([Spec 0078](docs/specs/0078-redactor-at-in-url-password.md)), one URL
shape where a password prefix stays visible
([ADR 0069](docs/adr/0069-redactor-at-in-url-credentials-decisions.md), §6),
and chains of three or more command-line secrets in one line
([ADR 0087](docs/adr/0087-command-line-password-redaction-boundaries.md)).

If you need the AI on such a server, use a **local** OpenAI-compatible
provider (e.g. Ollama on `127.0.0.1`): then nothing leaves your machine
([docs/netzwerkverbindungen.md](docs/netzwerkverbindungen.md)).

### What is deliberately not redacted

- **Commands** are shown, stored and sent as they are, so you always see
  exactly what runs. A secret the AI writes into a proposed command is
  visible to you in the confirmation dialog.
- **Filter decisions** in the ledger contain only text generated by the
  filter engine. The free-text reason of the AI second opinion is never
  stored for that reason
  ([ADR 0084](docs/adr/0084-red-risk-requires-confirm.md), §3).
- **Your own chat messages** are stored as you typed them (inside the
  encrypted database, section 3) and redacted only when they are sent.

---

## 3. Where data lives, and what is encrypted

The exact paths per platform are listed in
[docs/datenpfade.md](docs/datenpfade.md); the running app shows them under
**Settings → Diagnostics → Data paths**.

| Data | Location | Protection |
|---|---|---|
| Server profiles, groups, notes, rules, chat sessions, ledger, AI provider settings | Database `smart-ssh.db` | Whole file encrypted with SQLCipher |
| Secrets: SSH passwords, private keys, passphrases, certificates, sudo passwords, AI API keys, MCP token | Database `smart-ssh.db`, table `secrets` | Whole file encrypted with SQLCipher |
| Database key (root key) | OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service) — **or**, if you set a master password, a key file `smart-ssh.db.master-key` next to the database | Keychain: protected by the OS. Key file: root key wrapped with ChaCha20-Poly1305 under a key derived from your master password with Argon2id |
| Known host keys | `host_keys.json` next to the database | Plaintext (public keys and fingerprints) |
| App settings (language, layout, MCP on/off and shared servers, risk settings, notes and tags of the local pseudo-server) | `settings.json` | Plaintext, no secrets; the MCP token is not stored here |
| Logs | Log directory | Plaintext; no content at the default level (see below) |
| Edit copies of remote files you open in a local editor | User cache directory, one folder per data directory and session | Plaintext; owner-only permissions on Unix; removed when editing ends, the session disconnects, and at startup |
| Exports you save (e.g. diagnostics) | Wherever you save them | Plaintext, as you chose |

Backing documents:

- **Database encryption**: the whole database file is encrypted with
  SQLCipher, keyed with HKDF-SHA256 of a random 32-byte root key. No file
  outside the database holds host names, user names, header values, secrets
  or the MCP token in plaintext
  ([Spec 0101](docs/specs/0101-database-encryption.md),
  [ADR 0093](docs/adr/0093-database-encryption-startup-decisions.md),
  [ADR 0095](docs/adr/0095-master-password-wrapping-decisions.md),
  [`crates/persistence-sqlite/src/encryption.rs`](crates/persistence-sqlite/src/encryption.rs)).
- **Secrets in the database**: credentials live in the encrypted database,
  the OS keychain holds only the root key
  ([Spec 0096](docs/specs/0096-db-secrets-at-rest.md),
  [ADR 0088](docs/adr/0088-db-secrets-at-rest-decisions.md),
  [ADR 0094](docs/adr/0094-mcp-token-in-the-database.md)).
- **Chat content**: stored in the encrypted database. The earlier
  additional per-field encryption was removed in favour of whole-file
  encryption
  ([Spec 0036](docs/specs/0036-chat-content-encryption.md),
  [ADR 0112](docs/adr/0112-rueckbau-feldweise-verschluesselung.md)).
- **Linux without a Secret Service**: the app cannot use the keychain, so it
  asks you to set up a master password instead
  ([Spec 0101](docs/specs/0101-database-encryption.md)).
- **Logs**: at the default level (`info`), log lines carry no content — no
  commands, output, chat text or secrets. Content is logged only at `debug`,
  which is off unless you enable it explicitly, and is redacted there too
  ([Spec 0094](docs/specs/0094-no-content-in-default-logs.md),
  [ADR 0086](docs/adr/0086-content-only-on-debug-in-logs.md)).
- **Edit copies**: [ADR 0114](docs/adr/0114-editier-kopien-je-datenverzeichnis.md).
- **Host keys**: [ADR 0012](docs/adr/0012-file-host-key-store-and-event-extensions.md).

What encryption at rest protects against: someone who gets a copy of the
data directory (backup, stolen disk) without access to your unlocked OS
account or your master password. It does not protect against malware
running as your user while the app is unlocked.

---

## 4. Network connections

Smart SSH makes no network connection you did not trigger: no telemetry, no
update check, no license check, no fonts or scripts loaded from the
internet. The complete list — SSH, every AI provider call, the local Ollama
probe and the local MCP listener — with destination, trigger and whether
the connection leaves your machine is in
[docs/netzwerkverbindungen.md](docs/netzwerkverbindungen.md) (issue #14).

In short:

- **SSH** goes to the servers (and jump hosts) in your profiles, when you
  connect or test a connection.
- **AI calls** go only to the base URL of providers you configured. With a
  local provider they stay on your machine.
- **The Ollama probe** goes to `127.0.0.1:11434` only, once when the AI
  settings open and no Ollama provider exists.
- **The MCP server** is off by default; when enabled, it listens on
  `127.0.0.1` only and requires a bearer token
  ([Spec 0028](docs/specs/0028-mcp-server-integration.md)).

This is enforced by tests: the webview's Content Security Policy allows only
`connect-src 'self' ipc:`, and only an allowlisted crate may depend on an
HTTP client
([`apps/smart-ssh-community/tests/network_boundary.rs`](apps/smart-ssh-community/tests/network_boundary.rs)).

**Host keys.** Unknown host keys are never accepted silently: the
connection pauses and shows the fingerprint until you confirm it. A changed
host key is a hard stop with a strong warning
([Spec 0005](docs/specs/0005-ssh-module.md),
[Spec 0100](docs/specs/0100-host-key-dialog-keyboard.md)).

---

## 5. Trust boundaries that require your confirmation

Every action the AI proposes passes the filter engine first. Its decision is
`AutoExec`, `Confirm` or `Deny`. Without a matching allow rule the decision
is `Confirm`: **nothing the AI proposes runs automatically unless you
created an allow rule for it**
([`crates/core/src/filter/engine.rs`](crates/core/src/filter/engine.rs)).
Commands you type yourself in the terminal, and actions you trigger by hand
in the file browser, are yours and do not pass the filter
([Spec 0054](docs/specs/0054-sftp-browser-actions.md)).

The following cases require confirmation **even when an allow rule
matches**. Escalation only goes one way: these checks can turn `AutoExec`
into `Confirm`, never `Confirm` into `AutoExec`, and never lift a `Deny`
([`crates/app-logic/src/orchestration/action_exec.rs`](crates/app-logic/src/orchestration/action_exec.rs)).

| Boundary | Always? | Backed by |
|---|---|---|
| **MCP / external tools**: every action an external MCP client requests (run command, read/write file, note update). MCP is the interface for external tools | Always; not configurable | [Spec 0028](docs/specs/0028-mcp-server-integration.md), [Spec 0104](docs/specs/0104-mcp-sessions.md), [Spec 0039](docs/specs/0039-untrusted-content-fencing.md) |
| **File writes proposed by the AI** (SFTP write) | Always | [Spec 0020](docs/specs/0020-sftp-file-transfer.md), §4.2 |
| **Overwrite or delete in the file browser** (your own action) | Always | [Spec 0054](docs/specs/0054-sftp-browser-actions.md) |
| **Reading typical secret paths** (SSH keys, `.env`, …) | Always | [`action_exec.rs`](crates/app-logic/src/orchestration/action_exec.rs) |
| **Starting `sftp-server`** (elevated file browser) from an AI proposal | Always | [ADR 0058](docs/adr/0058-sftp-elevated-and-toasts-decisions.md), §8 |
| **Commands that use a stored sudo password** | Always | [`action_exec.rs`](crates/app-logic/src/orchestration/action_exec.rs) |
| **Suspected prompt injection** in content just read (if the injection check is enabled) | Always for the next action | [Spec 0039](docs/specs/0039-untrusted-content-fencing.md), §5 |
| **Any action after you rejected an earlier one in the same answer** | Always | [`action_exec.rs`](crates/app-logic/src/orchestration/action_exec.rs) |
| **Commands too long, too nested or too opaque to check** | Always | [ADR 0107](docs/adr/0107-filter-fail-closed-on-opaque-input.md) |
| **Hard-blacklisted commands** (e.g. `rm -rf /`) | Always | [`engine.rs`](crates/core/src/filter/engine.rs) |
| **Content read from the server** (post-ingest policy) | Per server: `Strict` always, `Balanced` for modifying actions, `Standard` never | [Spec 0039](docs/specs/0039-untrusted-content-fencing.md), [ADR 0027](docs/adr/0027-post-ingest-policy-scope-and-injection-check-timing.md) |
| **Red risk**: the risk indicator rates the proposal red on the server or the data axis — also when only the AI second opinion raises it to red | Setting "Always confirm on red risk", **on by default** | [Spec 0092](docs/specs/0092-red-risk-requires-confirm.md), [ADR 0084](docs/adr/0084-red-risk-requires-confirm.md) |

**About red risk.** The setting is on by default, and a missing or
unreadable value counts as on. With it on, a red-rated proposal never runs
without your confirmation, even against an allow rule. You can switch it
off in the AI settings; then red is shown as a warning only, from the next
connection on. If you edit a command in the confirmation dialog and run it,
your click is the confirmation: the edited text passes the filter engine
again, but not the red-risk check
([ADR 0084](docs/adr/0084-red-risk-requires-confirm.md), §7–§8).

---

## 6. Known residual risks

Risks we know about and accept, with the reason.

- **Redaction is pattern-based.** Unknown secret formats reach the AI
  provider (section 2). Accepted because a generic entropy fallback would
  break the copilot's usefulness without closing the gap
  ([Spec 0068](docs/specs/0068-security-hardening-prelaunch.md)).
  Mitigation: use a local AI provider for sensitive servers.
- **RSA timing side channel (RUSTSEC-2023-0071, "Marvin attack").** The
  `rsa` crate, pulled in by the SSH library, has a known timing side channel
  with no fixed release. Accepted because Smart SSH is a local client: RSA
  signing happens on your machine, and an attacker cannot trigger and time
  it repeatedly from outside. Replacing the SSH library would be the larger
  risk. Re-evaluated whenever `russh` or `rsa` update
  ([ADR 0028](docs/adr/0028-rsa-marvin-attack-risk-acceptance.md),
  [`.cargo/audit.toml`](.cargo/audit.toml)).
- **The risk classifier is pattern-based.** Its pattern lists are starting
  points without a claim to completeness. A dangerous command that matches
  no pattern is not rated red and, with an allow rule, runs automatically.
  The filter engine's deny rules and hard blacklist are unaffected
  ([ADR 0084](docs/adr/0084-red-risk-requires-confirm.md), §4).
- **Allow rules are your decision.** An allow rule lets matching proposals
  run without asking (except for the boundaries in section 5). A rule that
  is broader than intended widens what the AI can do on its own.
- **Prompt injection is mitigated, not solved.** Fencing, the optional
  injection check and the post-ingest policy reduce the risk that content on
  a server steers the AI. They cannot rule it out; the confirmation
  boundaries are the last line
  ([Spec 0039](docs/specs/0039-untrusted-content-fencing.md)).
- **The AI provider sees your session content.** With a cloud provider,
  redacted output, your messages and notes leave your machine and are
  subject to that provider's terms. The risk second opinion receives
  command text without redaction (section 1).
- **Plaintext outside the database.** `settings.json`, `host_keys.json`,
  edit copies and exports are not encrypted (section 3).
- **Unlocked app, compromised OS account.** While Smart SSH is running and
  unlocked, malware with your user's rights can read its memory or drive the
  UI. Encryption at rest does not help there.
- **Local MCP clients.** Any local process that has the MCP token can
  request actions on the servers you shared via MCP. Every such action still
  needs your confirmation (section 5)
  ([Spec 0028](docs/specs/0028-mcp-server-integration.md),
  [ADR 0094](docs/adr/0094-mcp-token-in-the-database.md)).
