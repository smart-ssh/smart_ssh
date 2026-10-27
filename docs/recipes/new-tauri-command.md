# Recipe: a new Tauri command

## Purpose

The frontend needs a new backend entry point (`invoke("name", …)`). Business
logic belongs in `crates/app-logic` or `crates/core`; the command only
translates (see `docs/architecture.md`).

## Files to touch

- [ ] `crates/app-shell/src/commands/<topic>.rs` – the `#[tauri::command]`
      function (pick the topic file that fits, e.g. `servers.rs`, `groups.rs`).
- [ ] `crates/app-shell/src/lib.rs` – add `commands::<name>` to the single
      `generate_handler!` list (`rg -n generate_handler crates/app-shell/src/lib.rs`).
- [ ] `crates/app-shell/src/startup_gate.rs` – only if the command must work
      while the app is locked: add it to `ALLOWED_WHILE_LOCKED`. Every other
      command is refused until unlock; do not add one without a reason.
- [ ] `apps/smart-ssh-community/frontend/src/api.ts` – typed wrapper around
      `invoke`.
- [ ] `apps/smart-ssh-community/frontend/src/types.ts` – argument/result
      types, mirroring the DTOs in `crates/app-logic/src/dto.rs` (serde
      `rename_all`, see the regression tests in `crates/app-logic/src/dto/`).
- [ ] `apps/smart-ssh-community/frontend/e2e/harness/fakeBackend.ts` – a
      handler, otherwise browser tests fail on the unknown command.
- [ ] Frontend tests that mock the API module via `vi.mock`: add the new
      function to those mocks (otherwise `undefined is not a function`).
- [ ] `apps/smart-ssh-community/capabilities/default.json` (and `mobile.json`)
      – normally **no change**: own `#[tauri::command]`s are allowed
      implicitly. Only plugin permissions are listed there.
- [ ] `changelog.d/issue-<n>-<topic>.md` and the spec of the area when
      user-visible behaviour changes.
- [ ] `THREAT-MODEL.md` if the command has side effects or touches a trust
      boundary.

## Required tests

- Rust: logic test next to the code in `crates/app-logic` / `crates/core`
  with trait mocks (`crates/app-logic/src/test_support.rs`).
- Frontend: a Vitest test for the calling component or the `api.ts` wrapper;
  `toHaveBeenCalledWith(...)` expectations updated when a parameter was added.
- Browser test (`apps/smart-ssh-community/frontend/e2e/tests/`) when a new
  screen or flow uses the command.

## Guard that catches a forgotten step

- `npm run check-commands` (`scripts/check-tauri-commands.mjs`, part of
  `npm run lint` in `apps/smart-ssh-community/frontend`) fails when a
  `#[tauri::command]` is not in `generate_handler!`, a `generate_handler!`
  entry has no definition, a non-test frontend file `invoke`s an unregistered
  name, or a name in `ALLOWED_WHILE_LOCKED` is not registered.
- Unknown commands in a browser test fail that test (missing `fakeBackend.ts`
  handler).
- No automated check: the `types.ts` ↔ DTO match, the `vi.mock` additions in
  older tests (surfaces as a test failure, not a named report), and whether the
  command belongs in `ALLOWED_WHILE_LOCKED`. The reviewer checks these.

## Example

PR #766 (`keep_server_as_regular_entry`, `keep_group_as_regular_entry`):
command in `crates/app-shell/src/commands/servers.rs` and `groups.rs`, registration in
`lib.rs`, wrapper in `api.ts`, types in `types.ts`, handler in `fakeBackend.ts`.
