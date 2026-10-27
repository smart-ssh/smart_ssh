# Glossary

Project and domain terms used across code, specs and issues. For UI wording
in translations see [`i18n-glossary.md`](i18n-glossary.md), which links back
here.

## Terms

**`A<n>` / `A<n>.<m>` (requirement IDs)** — Stable identifiers of numbered
requirements inside a spec (for example `A4.3`). Code, tests and comments
refer to them; an ID is kept as long as its statement holds and is never
reused. See `docs/specs/README.md`.

**Compaction** — Shrinking the context sent to the AI provider in long
sessions: a size threshold (measured after fencing) triggers it, the last
three rounds stay intact, and older rounds are condensed into a rolling
summary. Lives in `crates/app-logic/src/compaction.rs`; Spec 0057.

**Edition** — A concrete binary built on the shared crates. The community
build is `apps/smart-ssh-community`, a thin binary calling
`app_shell::run(Wiring::community())`; another app crate can embed the same
crates with a different `Wiring`. The `Edition` enum only affects display
and updates, never filter, risk or confirmation logic.
`crates/app-shell/src/wiring.rs`; Spec 0038.

**Elevated channel (elevated SFTP)** — A separate SFTP channel running as
another user (via sudo) that only the file browser can use. If it is
revoked or inactive, an action fails; it never falls back to the normal
channel. AI and MCP file actions never use it.
`crates/app-shell/src/elevated_sftp.rs`; Specs 0067, 0085.

**`BrowserAccess` / `BrowserChannel`** — Types that guard the elevated
channel. `BrowserAccess` is a token that can only be created inside the
`commands` module; `BrowserChannel` is the channel (normal or elevated)
handed to one file-browser command, which also records a mid-command
revocation. `crates/app-shell/src/commands/elevation.rs`; Specs 0084, 0085.

**Extension / extension registry** — A way for an embedding app to add UI
and behaviour without changing the core. In the frontend, extensions
register contributions (settings sections, form slots, session header
items, document actions) in
`apps/smart-ssh-community/frontend/src/extensions/registry.ts`; on the Rust
side they are supplied through `Wiring`.

**Extension tables** — Tables an extension keeps in the encrypted
application database under a unique name prefix, with its own migration
history. A SQLite authorizer rejects migrations that touch anything outside
the prefix. `crates/persistence-sqlite/src/extension_db.rs`; Spec 0038,
ADR 0139.

**`Feature` / `EntitlementProvider`** — `Feature` is the enum of gateable
features; `EntitlementProvider` is the trait that says which are currently
available (`current()` and `watch()`). The core only defines the
vocabulary; the community build uses `FixedEntitlements`.
`crates/core/src/entitlements.rs`; Spec 0037.

**Fence / fencing** — Wrapping content from untrusted sources (command
output, SFTP file contents, server and group notes) in escaped tags before
it enters any text sent to the AI, so it cannot pose as instructions.
Callers never build fence tags themselves.
`crates/core/src/ai/fencing.rs`; Spec 0039.

**Injection check** — A second-opinion call by the optional second
provider that looks at content read from a server for prompt-injection
attempts; the outcome can only raise caution for that server.
`crates/app-logic/src/second_opinion.rs` (`fetch_injection_check`);
Spec 0039, section 5.

**Ledger** — The append-only record of a session's events (entries are
written, never updated or deleted, except when the whole session is
deleted). Content is redacted before it is appended.
`crates/persistence-sqlite/src/ledger_store.rs`; Spec 0057.

**Local pseudo-server** — The always-present "Localhost" entry that runs
commands on the user's own machine through the same traits as a real
server, so filter, risk and confirmation apply identically.
`LOCAL_SERVER_ID` in `crates/app-logic/src/dto.rs`; Spec 0032.

**Redactor / redaction** — Masking secrets in command output and text with
placeholders before it is persisted, logged or sent to the AI, always and
for every provider. `OutputRedactor` in `crates/core/src/ai/redactor.rs`;
Spec 0006.

**Review priority ELEVATED** — A label on a change that touches
security-critical areas (filter engine, risk classifier, redaction,
persistence/migration); it asks the reviewer for extra adversarial test
cases. Marked in a spec's header as `Review-Priorität: ERHÖHT` (for
example Spec 0057).

**Second opinion** — An optional, separately configured AI provider that
re-assesses a command's data risk (and checks for injection). It can only
escalate a rating, never lower it.
`crates/core/src/ai/second_opinion.rs`,
`crates/app-logic/src/second_opinion.rs`; Specs 0026, 0043.

**Spec impact (`new` / `update NNNN` / `none`)** — Every change declares
how it affects specs: `new` adds a spec, `update NNNN` changes an existing
one, `none` is for refactoring, tests, CI or a fix of already specified
behaviour. See `AGENTS.md`, "Spec workflow".

**Startup gate / unlock** — Until the database is unlocked (master
password mode), only an allow list of commands (unlock, quit, start over)
is reachable; the gate sits in front of the command dispatcher so a new
command is blocked by default. `crates/app-shell/src/startup_gate.rs`;
Spec 0101, A16.

**`T<n>` (test-case IDs)** — Numbered test cases in a spec's table
(for example T1–T3 in Spec 0092); tests reference them by name.

**`Wiring`** — The struct a binary passes to `app_shell::run`: entitlement
provider, AI providers, policy sources, extra Tauri plugins and extension
registrations. `Wiring::community()` is the community default.
`crates/app-shell/src/wiring.rs`; Spec 0038.

## Historical terms

No longer used; they appear only in older documents and comments.

**BL-ID / `BL-NNNN`** — Backlog identifier of an item from an earlier
backlog. Issues are tracked on GitHub now.

**HQ** — An organisation directory that earlier agent workflows referred to
for escalations and questions. Not part of the current process.

**Teil 0 / Commit 1..n** — Structure of older task descriptions: first
clarify "part 0", then one commit per part. Only used when an issue
spells it out.
