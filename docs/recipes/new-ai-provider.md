# Recipe: a new AI provider type

## Purpose

A new value of `ProviderType` (a new kind of stored AI provider configuration
with its own wire format or its own rules), as opposed to a new model or base
URL for an existing type. Find every use first:

```bash
grep -rn 'ProviderType::' crates apps
```

Every file that list shows is covered below (production files in the
checklist, test-only files under "Required tests").

## Files to touch

- [ ] `crates/core/src/ai/types.rs` – the enum variant with its
      `#[serde(rename = ...)]`, `as_db_str` and `from_db_str`.
- [ ] A new migration: the `CHECK` constraint on `provider_type` in
      `crates/persistence-sqlite/migrations/0002_ai_provider_configs.sql`
      lists the allowed values, so the new value is rejected by the database
      until a migration rebuilds the table (follow `new-migration.md`; never
      edit `0002`).
- [ ] `crates/persistence-sqlite/src/ai_provider_store.rs` – reads the column
      with `ProviderType::from_db_str` and fails on unknown values; check that
      round trip.
- [ ] `crates/ai-providers/src/` – the provider implementation (a new module
      like `anthropic.rs` or `openai_compatible.rs`, `pub use` and `mod` in
      `crates/ai-providers/src/lib.rs`). It implements `AiProvider` and uses
      the shared pieces: `crates/ai-providers/src/retry.rs`, `crates/ai-providers/src/rate_limit_budget.rs`, `crates/ai-providers/src/usage.rs`,
      `crates/ai-providers/src/overload.rs`, `crates/ai-providers/src/request_logging.rs`, `crates/ai-providers/src/sse.rs`, `crates/ai-providers/src/content.rs`.
- [ ] `crates/ai-providers/src/discovery.rs` – model-list URL and auth header
      per type (two `match` sites).
- [ ] `crates/app-logic/src/ai_provider_factory.rs` – the `match` that builds
      the provider and its budget guard.
- [ ] `crates/app-logic/src/compaction.rs` – `model_context_window_tokens`
      needs a default window for the type.
- [ ] `crates/app-logic/src/backup/validate.rs` – a backup file with the new
      value must pass validation (uses `from_db_str`, so usually nothing to
      change; verify with a test).
- [ ] `crates/app-logic/src/dto.rs` and `crates/app-logic/src/diagnostics.rs` –
      DTOs carry the type; diagnostics export names it by `as_db_str`.
- [ ] `crates/app-logic/src/ai_usage.rs` – usage records carry the type.
- [ ] `crates/app-shell/src/commands/ai_providers.rs` – the `match` blocks for
      base-URL requirement, key requirement, model discovery and connection
      test.
- [ ] `crates/app-shell/src/commands/connect.rs` – builds the session from the
      active configuration (context window, provider label).
- [ ] Frontend: `apps/smart-ssh-community/frontend/src/types.ts`
      (`ProviderType` union, `needsBaseUrl`, `supportsWebResearch`,
      `supportsModelDiscovery`),
      `apps/smart-ssh-community/frontend/src/components/AddAiProviderDialog.tsx`
      (option and defaults),
      `apps/smart-ssh-community/frontend/src/apiKeyFormat.ts` (key prefix
      check) and the texts per `new-ui-text.md`.
- [ ] `docs/specs/` – the provider specs (`docs/specs/0006-ai-provider-trait.md`,
      `docs/specs/0025-extended-openai-compatible-providers.md`) when behaviour changes;
      `docs/netzwerkverbindungen.md` and `THREAT-MODEL.md` (new outgoing
      host, what is sent), `changelog.d/issue-<n>-<topic>.md`.

### Security-relevant step: fencing and redaction

Everything remote that reaches the model (command output, read files, notes,
the system identifier) is fenced and redacted before it is sent
(`.claude/skills/smart-ssh-coder/references/security-invariants.md`,
items 8 and 11). The new provider must go through the same path:

- Build request messages from the already sealed content, as
  `crates/ai-providers/src/anthropic.rs` and
  `crates/ai-providers/src/openai_compatible.rs` do (see
  `crates/ai-providers/src/content.rs`); never concatenate raw output.
- Use `fence_untrusted` and the fencing helpers in
  `crates/core/src/ai/fencing.rs` for any text built inside the provider
  (placeholders, tool results). Escape fence markers in untrusted text first.
- Do not add a second path that sends stored history without
  `reapply_redaction_for_send` (called by the core before `AiProvider::send`).
- Never log request bodies or keys outside `crates/ai-providers/src/request_logging.rs`, which
  redacts.

## Required tests

- Wiremock tests in `crates/ai-providers/tests/` (pattern:
  `anthropic.rs`, `openai_compatible.rs`, `openai_responses.rs`): streaming,
  tool calls, error classification, retry, token usage, and a test that a
  planted secret and a fence-marker string in server output never reach the
  request body unredacted/unescaped.
- Discovery tests in `crates/ai-providers/src/discovery.rs`, factory tests in
  `crates/app-logic/src/ai_provider_factory.rs`, context window test in
  `crates/app-logic/src/compaction.rs`.
- Store round trip in `crates/persistence-sqlite/src/ai_provider_store.rs`
  and the migration test from `new-migration.md`.
- Test-only users of `ProviderType::` that need the new variant in any
  exhaustive `match` or fixture: `crates/persistence-sqlite/src/tests_encryption.rs`,
  `crates/persistence-sqlite/src/tests_data_changes.rs`,
  `crates/persistence-sqlite/src/tests_fixture_t0.rs`,
  `crates/persistence-sqlite/tests/fixtures/releases/generate_v0.5.2.rs`
  (a historical generator: leave it unchanged),
  `crates/app-logic/src/dto/tests.rs`,
  `crates/app-logic/src/secret_migration/tests.rs`,
  `crates/app-logic/src/orchestration/chat_turn/tests_usage.rs`,
  `crates/app-shell/src/commands/connect/tests_flow.rs`.
- Frontend: Vitest for `apiKeyFormat.ts` and `types.ts` helpers.

## Guard that catches a forgotten step

- A `match` on `ProviderType` without a wildcard arm fails to compile
  (`cargo check --workspace --all-targets`); that covers the factory,
  discovery, compaction and the commands. A `_ =>` arm hides the gap: do not
  add one.
- The database `CHECK` constraint rejects the new value until the migration
  exists; the store test fails.
- `npx tsc -b` fails for an exhaustive switch over the `ProviderType` union
  only where the code is written that way.
- No automated check: the frontend dialog and key-format function (a missing
  option is simply absent), the fencing/redaction behaviour of the new
  provider (only the tests you write), the security docs. The reviewer checks
  these.

## Example

No provider type has been added since the four existing ones. The closest
merged change is PR #240 (web research for the official OpenAI provider, a
new `OpenAiResponsesProvider` behind the existing `openai` type with its own
wire format, factory branch, tests in `crates/ai-providers/tests/` and
fencing through the shared path). Use it for the provider module and its
tests; the enum, migration and frontend steps are described above.
