-- Issue #504 (ADR 0141): new message kind `extension` (opaque reference to
-- extension-owned content attached to a user message, stored like every
-- other message encrypted in `content`). The `CHECK` constraint on
-- `content_type` cannot be altered in SQLite, so the same technique as in
-- `0009_chat_messages_content_blob.sql` and `0021_...`: recreate the table,
-- copy the rows unchanged, rename, recreate the index. Migration 0021 stays
-- byte-identical (sqlx checks checksums); `attachments` stays allowed here.

PRAGMA foreign_keys = OFF;

CREATE TABLE chat_messages_new (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'action_result')),
    content_type    TEXT NOT NULL CHECK (
        content_type IN ('text', 'command_result', 'action_rejected', 'document', 'web_activity', 'attachments', 'extension')
    ),
    content         BLOB NOT NULL,   -- nonce || ciphertext (Spec 0036, Abschnitt 3)
    sequence        INTEGER NOT NULL,
    created_at      TEXT NOT NULL
);

INSERT INTO chat_messages_new (id, session_id, role, content_type, content, sequence, created_at)
SELECT id, session_id, role, content_type, content, sequence, created_at FROM chat_messages;

DROP TABLE chat_messages;
ALTER TABLE chat_messages_new RENAME TO chat_messages;

CREATE INDEX idx_chat_messages_session ON chat_messages(session_id, sequence);

PRAGMA foreign_keys = ON;
