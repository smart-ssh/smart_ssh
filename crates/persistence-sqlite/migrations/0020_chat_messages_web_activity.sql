-- Issue #162: neue Nachrichtenart `web_activity` (serverseitige
-- Web-Recherche des KI-Providers, gespeichert wie jede andere Nachricht
-- verschlüsselt in `content`). Die `CHECK`-Constraint auf `content_type`
-- lässt sich in SQLite nicht ändern, daher dieselbe Technik wie in
-- `0009_chat_messages_content_blob.sql`: Tabelle neu anlegen, Zeilen
-- unverändert kopieren, umbenennen, Index neu anlegen. Spalten, Typen und
-- alle übrigen Constraints bleiben exakt wie in 0009.

PRAGMA foreign_keys = OFF;

CREATE TABLE chat_messages_new (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'action_result')),
    content_type    TEXT NOT NULL CHECK (
        content_type IN ('text', 'command_result', 'action_rejected', 'document', 'web_activity')
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
