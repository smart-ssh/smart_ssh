# Extension reads the activity record

## 1. Purpose

An extension needs recorded sessions or ledger entries (across servers, with
filters and paging), or the read API gets a new filter or field. Extensions
use the functions in `app_logic::ext`; they never query core tables with raw
SQL through `ExtensionDb::pool()`.

## 2. Files to touch

- [ ] `crates/persistence-sqlite/src/record_query.rs` — the SQL (filter, keyset paging by insertion order, `LIMIT`); the only reader that knows the table layout.
- [ ] `crates/app-logic/src/record_query.rs` — the DTOs and filters that extensions see (independent of the table layout) and the mapping.
- [ ] `crates/app-logic/src/ext.rs` — the function and the re-export of new types (the record-query module itself stays private).
- [ ] `crates/persistence-sqlite/src/tests_record_query.rs` — a test per filter and a combined one.
- [ ] `THREAT-MODEL.md` — section "The activity record and who can read it" if what can be read changes.
- [ ] `docs/codemap.md` — the extension-point entry for the `ext` facade.

## 3. Required tests

- Each new filter narrows the result alone and combined with others.
- Paging returns every entry once while entries are appended.
- Chat message content is never part of a result.
- A damaged row (content, timestamp, rule snapshot, session id, unknown source) is returned as an item with `unreadable` set, in its place; it never fails a page or a session read, and a new mapped column is damage-tested too.
- A session read stays chunked (one bounded statement per chunk), so appends are not blocked.
- The row mapping is shared with `SqliteLedgerStore::load_entries` (`ledger_entry_from_row`); add new columns there.
- Text that leaves the device is redacted with `redact_for_server`.
- With a text filter a page can be short or empty while `next_cursor` is still set (the scan per call is bounded); keep paging until `next_cursor` is `None`.

## 4. Guard that catches a forgotten step

`test_record_query_is_reachable_through_ext_only` in `crates/app-logic/src/record_query.rs` fails when the module becomes public or a function is missing from `ext`. There is no guard for the threat-model section; the review checks it.

## 5. Example

Issue #1449 (the read API itself).
