-- Issue #673: namespaced key-value and secret store for integrations.
-- Additive: one new table. The `ext_` name prefix keeps it out of backups
-- (Spec 0109, C2). Values (including secrets) live in plaintext inside the
-- encrypted database file, like `secrets`.
CREATE TABLE ext_kv (
    namespace  TEXT NOT NULL,
    key        TEXT NOT NULL,
    value      BLOB NOT NULL,
    is_secret  INTEGER NOT NULL DEFAULT 0 CHECK (is_secret IN (0, 1)),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (namespace, key)
);
