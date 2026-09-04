# smart_ssh — CLAUDE.md

Cross-platform SSH client (Tauri 2 + Rust core + React frontend) with an
AI copilot that suggests commands, which run through a policy/filter
engine before anything touches a real server. The project's core promise
is **full transparency and control over every command that reaches a
server** — the AI is a copilot, never an autonomous actor. Code quality
rules here exist to protect that promise, not as bureaucracy.

## Before every commit

Run the full gate and make sure it's green — this mirrors CI
(`.github/workflows/community.yml`, Spec 0038) plus the frontend lint/test
checks CI does *not* yet run (it does run a frontend build, i.e. `tsc -b &&
vite build`, since Spec 0038), so don't rely on CI alone to catch oxlint/
vitest regressions:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

```bash
cd apps/smart-ssh-community/frontend
npx tsc -b
npx oxlint
npx vitest run
```

Order matters: `cargo fmt --all --check` runs *before* clippy/test in CI —
if formatting is off, clippy and test never even execute, so a red CI run
that only shows a `fmt` failure tells you nothing about whether the actual
code is correct. Run `cargo fmt --all` locally before committing, not just
`--check`.

For UI-affecting changes, also start the dev app and click through the
golden path once — type-checking and unit tests verify code correctness,
not feature correctness. On macOS use `./scripts/tauri-dev.sh`, not plain
`cargo tauri dev` (a code-signing/keychain quirk, see
`docs/adr/0022-stable-dev-code-signature.md`). Don't relaunch it
repeatedly while iterating — build/test/lint don't need a running window;
start it once, when a change is actually ready to look at.

## Architecture rules

```
crates/
├── core/               # pure logic — NO Tauri, NO UI framework dependency
│   ├── ssh/              # connection handling, trait-based
│   ├── filter/            # policy/filter engine (security-critical)
│   ├── risk/               # command risk classification
│   ├── ai/                  # AI provider abstraction + output redaction
│   └── profiles/              # server/group/credential domain types
├── ssh-transport/       # concrete SshTransport impl (russh + local pseudo-server)
├── persistence-sqlite/  # concrete ProfileStore/PolicyStore impl
├── credentials-keyring/ # concrete CredentialStore impl (OS keychain)
├── ai-providers/        # concrete AiProvider impls (Anthropic, OpenAI-compatible, ...)
├── mcp-server/           # MCP server exposing sessions to external clients
└── app-shell/            # thin wrapper: Tauri commands/events <-> core APIs,
                           # edition-parametrized via `Wiring` (Spec 0038)
apps/
└── smart-ssh-community/  # thin binary: app_shell::run(Wiring::community())
    └── frontend/           # React, talks only to app-shell via invoke()
```

**`core` never depends on Tauri or a UI framework.** All domain logic
belongs there; `app-shell` only translates between Tauri commands/events
and `core` APIs. This is what lets a future TUI reuse the same logic
without duplication — don't put business logic in `app-shell` because it
was faster to reach `AppHandle`/`State` from there.

**Every external boundary is a trait, defined before its concrete
implementation.** `SshTransport`, `SftpSession`, `InteractiveShell`,
`AiProvider`, `PolicyStore`, `ProfileStore`, `CredentialStore` — each has
an in-memory/mock implementation used in tests, so `core`'s logic (above
all the filter engine) is fully testable without a real SSH connection or
API call. When adding a new external dependency, add the trait first.

**No special-casing the local pseudo-server (or any other server) in the
core loop.** The local pseudo-server (`ssh-transport::LocalTransport`,
`app_shell::local_server`) exists specifically to prove this: it
implements the same traits as a real server, so the filter engine, risk
classifier, and confirmation flow apply to it identically, with zero
branching on server identity anywhere in that path. If you find yourself
writing `if server_id == LOCAL_SERVER_ID` inside filter/risk/confirmation
code, that's a sign something is architecturally wrong — the special
casing belongs at the transport/session-construction boundary, never
inside the security logic itself.

## Security-critical modules — extra care

`crates/core/src/filter/` (policy engine), `crates/core/src/risk/` (risk
classifier), `crates/core/src/ai/` output redaction, and credential
handling (`credentials-keyring`, `crates/core/src/profiles/credentials.rs`)
are the modules standing between an AI suggestion and something
irreversible happening on a real server. For changes here:

- Never loosen an existing check to make a test pass or a feature work —
  find out *why* the check exists (usually a numbered spec test case, see
  `crates/core/src/filter/tests.rs`'s `test_t*`/named cases) before
  touching it.
- Escalation only goes one direction: a rule-based `Deny`/risk-`Red` must
  never be downgraded by an AI opinion or a new code path (see
  `docs/adr/0024-risk-indicator-second-opinion-design.md` for the pattern:
  "only escalation, never softening").
- Add adversarial test cases, not just happy-path ones — evasion via
  command chaining, substitution, encoding, or a sufficiently long input
  are the standard categories already covered in
  `crates/core/src/filter/tests.rs`; extend from there rather than
  starting over.
- Redaction (`crates/core/src/ai/`) must run before content is persisted
  or sent anywhere — verify with a test that plants a redaction-worthy
  secret and asserts it never appears in plaintext in the output/DB.

## Spec-first workflow

Every domain module gets a numbered spec in `docs/specs/` (`NNNN-slug.md`)
before code is written — check the next free number and read at least the
specs it explicitly references before implementing. `docs/adr/` holds
architecture decisions that cut across a single spec, or non-obvious
implementation choices made while executing one (numbered independently,
see `docs/adr/README.md`); write one whenever you make a call the spec
left open, especially a scope reduction (e.g. "X works but without
history/feature Y, because Z") — say so explicitly rather than silently
narrowing what was asked for.

## Testing conventions

- Rust: tests live in `#[cfg(test)] mod tests` next to the code, using
  trait mocks (`MockAiProvider` in `crates/core/src/ai/tests.rs`, the
  in-memory `ProfileStore`/`CredentialStore` in
  `crates/app-shell/src/test_support.rs`) — no real network/SSH/keychain
  access in unit tests. `crates/ssh-transport/tests/integration.rs` is the
  one place real (test-fixture) SSH connections are exercised.
- Frontend: pure logic (`groupTree.ts`, `remotePath.ts`, `errorCodes.ts`, …)
  gets its own `*.test.ts` via Vitest; component tests exist for the
  trickier interactive pieces, not for every component.
- When a bug is fixed, add the regression test that would have caught it,
  not just the fix — see `crates/app-shell/src/dto.rs`'s
  `rename_all_fields` regression tests for the pattern.

## Commits

Conventional Commits, scoped to the crate/area, referencing the spec:
`feat(app-shell): add X per spec 0032`, `fix(ci): ...`,
`docs(adr): propose design decisions for X (spec 0032)`. Keep the ADR as
a separate commit after the feature commit it documents. Only commit when
asked; run the full gate above first.


## Verbindlicher Review-Workflow nach jedem Spec-Implementierungsschritt

Nach dem letzten Commit eines Implementierungsschritts (oder eines
benannten Teils davon, z. B. "Teil 1"), **bevor** du dem Nutzer den
Abschluss meldest:

1. Rufe explizit den `spec-reviewer`-Subagenten auf (Task-Tool, Agent
   `spec-reviewer`) — verlass dich nicht auf automatisches Delegieren,
   ruf ihn aktiv auf. Gib ihm mit: den Pfad zur betroffenen Spec
   (`docs/specs/00XX-*.md`), die Commit-Range seit Beginn dieses Schritts,
   und die Priorität ("NORMAL" oder "ERHÖHT" — ERHÖHT, wenn der Schritt
   Filter-Engine, Risiko-Klassifizierer, Redactor oder Credential-Handling
   berührt).
2. Lies den zurückgelieferten Bericht vollständig.
3. Für jeden gefundenen Punkt: entscheide, ob du ihn direkt behebst.
   - Behebst du ihn: fixe, lass Tests/Clippy erneut grün laufen, committe
     den Fix separat mit Bezug auf den Review-Fund.
   - Behebst du ihn **nicht** (z. B. weil du ihn für einen Fehlalarm
     hältst, für außerhalb des Scopes dieses Schritts, oder für eine
     bewusste Design-Entscheidung): **liste das dem Nutzer explizit auf,
     mit Begründung, warum du es nicht angefasst hast.** Nichts aus dem
     Bericht stillschweigend fallen lassen.
4. Abschlussmeldung an den Nutzer enthält immer beide Teile: was durch den
   Review gefunden und behoben wurde, und was gefunden, aber bewusst nicht
   behoben wurde (mit Begründung).

Dieser Workflow ist nicht optional und nicht nur bei offensichtlich
riskanten Änderungen anzuwenden — er gilt nach jedem Implementierungsschritt
mit eigenem Commit.