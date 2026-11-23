-- Issue #1506: the core keeps no activity record. Session activity is only
-- available to extensions through the activity observer hook. Existing
-- ledger and recorded-session data is discarded on purpose.
--
-- `chat_sessions` (parent of `chat_messages`) is NOT rebuilt: sqlx runs a
-- migration in a transaction where `PRAGMA foreign_keys = OFF` has no effect,
-- so dropping it would cascade into its children. Only the plain column
-- (no index, no foreign key) is dropped.

DROP TABLE ledger_entries;
DROP TABLE activity_sessions;
ALTER TABLE chat_sessions DROP COLUMN activity_session_id;
