# Recipe: a new database migration

## Purpose

The SQLite schema changes (new table, column or index). Released databases
must upgrade without losing any row; migrations are append-only.

## Files to touch

- [ ] `crates/persistence-sqlite/migrations/NNNN_<name>.sql` – the next free
      number (`ls crates/persistence-sqlite/migrations`). Never edit an
      existing migration. Check open PRs for the same number first.
- [ ] `crates/persistence-sqlite/src/store.rs` – store code: for a new
      `servers` column, both SELECTs, the INSERT, the full UPDATE and
      `row_to_server`
      (`grep -nE 'FROM servers|INSERT INTO servers|UPDATE servers|fn row_to_server' crates/persistence-sqlite/src/store.rs`).
- [ ] `crates/persistence-sqlite/src/chat_session_store.rs` – the assertion on
      the list of applied migration versions
      (`(1..=N).collect::<Vec<i64>>()`) goes up by one.
- [ ] Struct literals of any changed domain type across crates
      (`cargo check --workspace --all-targets` lists each one).
- [ ] DTO in `crates/app-logic/src/dto.rs` and frontend type in
      `apps/smart-ssh-community/frontend/src/types.ts` when the field is
      exposed.
- [ ] `crates/persistence-sqlite/tests/fixtures/releases/` and its `README.md`
      – only when a *release* ships a new schema (a new fixture is written by
      the released build, see the README). Not needed per migration.
- [ ] `docs/datenpfade.md` / `THREAT-MODEL.md` if new kinds of data are stored.
- [ ] `changelog.d/issue-<n>-<topic>.md` when user-visible.

## Required tests

- A migration test that creates data in the **old** schema with raw SQL, runs
  the migration and checks the data (pattern:
  `test_migration_from_earlier_schema_with_real_data_preserves_all_rows` in
  `crates/persistence-sqlite/src/tests.rs`). New columns must not appear in
  the old-schema SQL.
- Store round-trip test for the new field.
- The release-chain tests must stay green:
  `crates/persistence-sqlite/src/tests_release_chain.rs` (every release
  fixture upgraded migration by migration, no table/column/row lost) and
  `crates/app-logic/src/database_startup/tests_release_upgrade.rs`.

## Guard that catches a forgotten step

- `crates/persistence-sqlite/src/tests_release_chain.rs` fails when a
  migration drops or changes data of a released database.
- The version-list assertion in `chat_session_store.rs` fails when the number
  of migrations changes without the test being updated.
- `cargo check --workspace --all-targets` reports every struct literal missing
  a new field without a default.
- No automated check: missing columns in a hand-written SQL statement in
  `store.rs` (an omitted column in a SELECT/UPDATE surfaces only through a
  round-trip test you wrote), and a migration number clash with an open PR.
  The reviewer checks these.

## Example

PR #766: `crates/persistence-sqlite/migrations/0026_copy_marker.sql`, store and
backup-store changes, and the version assertion bumped from 25 to 26.
