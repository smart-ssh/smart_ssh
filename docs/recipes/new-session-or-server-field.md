# Recipe: a new `Session` or `Server` field

## Purpose

A new field on the in-memory `Session` or on the stored `Server` profile.
Both are built with struct literals in many places (production code, tests,
`test_support`), and the `Server` field also travels through the database,
backup and import, the DTO and the frontend.

## Files to touch

### `Session` field

- [ ] The struct and every literal. Do not count them from memory, list them:

  ```bash
  grep -rnE '\bSession \{' --include='*.rs' crates apps | grep -v -e '->' -e 'struct ' -e 'impl '
  ```

  This returns the struct literals (production, tests, `test_support`)
  without signatures and other types. Every site gets the field, then
  `cargo check --workspace --all-targets`.
- [ ] `crates/app-shell/src/commands/connect.rs` – where the session is
      built at connect time.

### `Server` field

- [ ] `crates/core/src/profiles/` – the `Server` type (and validation if the
      value is user-entered).
- [ ] A migration (`new-migration.md`): a nullable column or one with a
      default.
- [ ] `crates/persistence-sqlite/src/store.rs` – both SELECTs, the INSERT,
      the complete UPDATE and `row_to_server`:

  ```bash
  grep -nE 'FROM servers|INSERT INTO servers|UPDATE servers|fn row_to_server' crates/persistence-sqlite/src/store.rs
  ```

- [ ] `crates/persistence-sqlite/src/backup_store.rs` – `ServerRow`, the
      snapshot (`SELECT * FROM servers` is mapped field by field) and the
      restore `INSERT … ON CONFLICT` statement. Plus the backup file format
      in `crates/app-logic/src/backup/` when the field is part of the file
      (keep it optional so older files still import).
- [ ] `crates/app-logic/src/dto.rs` – `ServerDto` and `ServerInput`;
      `crates/app-logic/src/servers.rs` – create/update.
- [ ] Struct literals of `Server` and `ServerInput` across crates, examples
      and tests. If the field has no default, `cargo check --workspace
      --all-targets` shows every missing site; insert it **after the last
      field** of the literal, not after a named one (the order changes with
      every migration).
- [ ] Frontend: `apps/smart-ssh-community/frontend/src/types.ts` (`Server`
      and `ServerInput`), the server form, and every test fixture that builds
      a server; texts per `new-ui-text.md`.
- [ ] Importers/exporters that list server fields (for example the
      `ssh_config` export names fields without an OpenSSH equivalent).
- [ ] `docs/specs/` (profile spec `docs/specs/0003-server-profile-datenmodell.md` or the
      spec of the feature), `changelog.d/issue-<n>-<topic>.md`.

## Required tests

- Store round trip for the new field (pattern: `test_start_directory_roundtrip`
  in `crates/persistence-sqlite/src/tests.rs`).
- Backup round trip including the optional-in-file case (pattern:
  `copy_marker_round_trips_and_is_optional_in_the_file` in
  `crates/app-logic/src/backup/tests.rs`).
- The migration test from `new-migration.md`; the old-schema SQL in
  `test_migration_from_earlier_schema_…` creates data in the **old** schema
  with raw SQL, so the new column must not appear there.
- Validation tests when the value is user-entered.

## Guard that catches a forgotten step

- `cargo check --workspace --all-targets` reports every struct literal that
  lacks a field without a default.
- The release-chain tests (`crates/persistence-sqlite/src/tests_release_chain.rs`)
  fail when a migration loses data of a released database.
- **Not detected today:** a column missing from a hand-written SQL statement
  in `store.rs` or `crates/persistence-sqlite/src/backup_store.rs`. Nothing compares the column list of
  `servers` with these statements; the field is silently dropped (stored as
  `NULL`/default, lost on backup restore) unless your round-trip tests above
  cover it. A forgotten frontend fixture or form field is likewise caught
  only by `npx tsc -b` when the type makes the field required.

## Example

PR #21 added `start_directory` to the server profile: migration `0017`, core
type, DTO and input, store, frontend form and fixtures, spec 0102
(`docs/specs/0102-server-start-directory.md`). PR #766 added `is_copy`
(migration `crates/persistence-sqlite/migrations/0026_copy_marker.sql`) with the backup-store change and the
backup round-trip test.
