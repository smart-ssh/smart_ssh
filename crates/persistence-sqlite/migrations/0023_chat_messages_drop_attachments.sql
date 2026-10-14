-- Issue #507 (ADR 0141): the built-in attachment model is gone; content
-- attached to chat messages exists only as `extension` references. Remove
-- `attachments` from the `content_type` constraint. Rows of that type were
-- only written by unreleased development builds and are deleted. The
-- `CHECK` constraint cannot be altered in SQLite, so the same technique as
-- in `0022_...`: recreate the table, copy the remaining rows unchanged,
-- rename, recreate the index. Migrations 0021 and 0022 stay byte-identical
-- (sqlx checks checksums).

PRAGMA foreign_keys = OFF;

CREATE TABLE chat_messages_new (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'action_result')),
    content_type    TEXT NOT NULL CHECK (
        content_type IN ('text', 'command_result', 'action_rejected', 'document', 'web_activity', 'extension')
    ),
    content         BLOB NOT NULL,   -- nonce || ciphertext (Spec 0036, Abschnitt 3)
    sequence        INTEGER NOT NULL,
    created_at      TEXT NOT NULL
);

INSERT INTO chat_messages_new (id, session_id, role, content_type, content, sequence, created_at)
SELECT id, session_id, role, content_type, content, sequence, created_at FROM chat_messages
WHERE content_type != 'attachments';

DROP TABLE chat_messages;
ALTER TABLE chat_messages_new RENAME TO chat_messages;

CREATE INDEX idx_chat_messages_session ON chat_messages(session_id, sequence);

PRAGMA foreign_keys = ON;
