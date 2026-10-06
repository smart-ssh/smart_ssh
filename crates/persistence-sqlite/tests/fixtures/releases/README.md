# Release database fixtures

One database file per released schema, each **written by the released build
itself**. `src/tests_release_chain.rs` upgrades every file here to the
current build one migration at a time and checks after each step that no
table, column or row the file held has gone or changed; at the end it opens
the file through `SqliteProfileStore::connect_encrypted` and reads the data
back through the stores. `app-logic` uses the newest fixture for the
repeated-start test (`database_startup/tests_release_upgrade.rs`).

| File | Release | Schema (highest migration) | Encryption |
|---|---|---|---|
| `v0.5.2.sqlite3` | 0.5.2 | 14 | plaintext (before SQLCipher) |

The registry is `RELEASE_FIXTURES` in `src/test_support.rs`;
`test_release_fixtures_are_registered_consistently` checks that every entry
matches its file.

Every fixture uses the same fixed, non-secret root key
`RELEASE_FIXTURE_ROOT_KEY` (`src/test_support.rs`) for its field-encrypted
content (chat, prompt history, ledger) and, from SQLCipher on, as the K its
database key is derived from.

## Adding a fixture when a release is cut

Do this for every release whose schema differs from the newest fixture's
(i.e. the release ships a migration that no fixture carries yet). Releases
without a new migration need no fixture.

1. Check out the release tag in a separate worktree, e.g.
   `git worktree add --detach target/fixture-vX.Y.Z vX.Y.Z`.
2. In that worktree, copy the newest `generate_v*.rs` from this directory to
   `crates/persistence-sqlite/src/gen_release_fixture.rs` and register it in
   `src/lib.rs` as `#[cfg(test)] mod gen_release_fixture;`.
3. Adapt the generator to the release's API and **keep every existing
   marker** (`*-r052` values, row counts) so the chain test's checks still
   hold. For tables or columns the release added, write rows with new
   markers of the form `*-rXYZ`.
   - From SQLCipher on (0.6.0 and later), open the file the way the release
     does: `SqliteProfileStore::connect_encrypted(&out,
     &DatabaseKey::from_root_key(&RELEASE_FIXTURE_ROOT_KEY))` instead of
     the plaintext `connect`.
   - Secrets that the release keeps in the database (`secrets` table) get
     marker values too.
4. Run it once in the worktree:
   `FIXTURE_OUT=<absolute path>/vX.Y.Z.sqlite3 cargo test -p persistence-sqlite --lib -- --ignored generate_release_fixture`
5. Copy the file here as `vX.Y.Z.sqlite3` and the generator as
   `generate_vX.Y.Z.rs` (add the "not compiled in this tree" header).
6. Append an entry to `RELEASE_FIXTURES` in `src/test_support.rs` (release,
   file name, highest migration of the release, `Plaintext`/`Sqlcipher`),
   extend the table above, and add checks for the new markers to
   `assert_release_data_readable` in `src/tests_release_chain.rs`.
7. Run `cargo test -p persistence-sqlite --lib release` and remove the
   worktree.

Never regenerate an existing fixture from a later build: the point of the
file is that the release wrote it.
