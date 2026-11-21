-- Spec 0057 (activity record), issue #1443: a recorded session that exists
-- independently of a chat, and a ledger table that can hold every entry type.
--
-- Parent tables (`chat_sessions`, `servers`, `groups`) are NOT rebuilt: sqlx
-- runs a migration in a transaction where `PRAGMA foreign_keys = OFF` has no
-- effect, so `DROP TABLE` of a parent would cascade into its children. Only
-- the child `ledger_entries` is rebuilt; `chat_sessions` gets one plain
-- `ADD COLUMN`.

CREATE TABLE activity_sessions (
    id               TEXT PRIMARY KEY,
    kind             TEXT NOT NULL CHECK (kind IN ('chat', 'mcp', 'terminal')),
    mcp_client       TEXT,
    -- Deliberately no foreign key: the snapshot below keeps the record
    -- readable after the server is deleted.
    server_id        TEXT NOT NULL,
    server_name      TEXT NOT NULL,
    server_host      TEXT NOT NULL,
    server_port      INTEGER NOT NULL,
    server_user      TEXT NOT NULL,
    -- JSON arrays, root group first.
    group_ids        TEXT NOT NULL DEFAULT '[]',
    group_names      TEXT NOT NULL DEFAULT '[]',
    tags             TEXT NOT NULL DEFAULT '[]',
    -- IANA name of the device time zone; NULL = unknown (backfilled rows).
    time_zone        TEXT,
    started_at       TEXT NOT NULL,
    ended_at         TEXT,
    last_activity_at TEXT,
    failed_writes    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_activity_sessions_server ON activity_sessions(server_id, started_at);

ALTER TABLE chat_sessions ADD COLUMN activity_session_id TEXT;

-- Backfill: one recorded session per existing chat session. The recorded
-- session reuses the chat session id, so existing ledger rows keep their
-- `session_id` unchanged.
WITH RECURSIVE path(server_id, gid, name, parent_id, depth) AS (
    SELECT s.id, g.id, g.name, g.parent_id, 0
    FROM servers s JOIN groups g ON g.id = s.group_id
    UNION ALL
    SELECT p.server_id, g.id, g.name, g.parent_id, p.depth + 1
    FROM path p JOIN groups g ON g.id = p.parent_id
)
INSERT INTO activity_sessions (
    id, kind, mcp_client, server_id, server_name, server_host, server_port,
    server_user, group_ids, group_names, tags, time_zone, started_at, ended_at,
    last_activity_at, failed_writes
)
SELECT
    c.id, 'chat', NULL, s.id, s.name, s.host, s.port, s.username,
    COALESCE((SELECT json_group_array(gid) FROM
        (SELECT gid FROM path WHERE server_id = s.id ORDER BY depth DESC)), '[]'),
    COALESCE((SELECT json_group_array(name) FROM
        (SELECT name FROM path WHERE server_id = s.id ORDER BY depth DESC)), '[]'),
    COALESCE((SELECT json_group_array(tag) FROM
        (SELECT tag FROM server_tags WHERE server_id = s.id ORDER BY tag)), '[]'),
    NULL, c.started_at, c.ended_at, NULL, 0
FROM chat_sessions c JOIN servers s ON s.id = c.server_id;

UPDATE chat_sessions SET activity_session_id = id;

-- Rebuild of the child table `ledger_entries`: `session_id` now references
-- the recorded session (no cascade from chats or servers), every entry type is
-- allowed, new nullable columns.
CREATE TABLE ledger_entries_new (
    id                    TEXT PRIMARY KEY,
    session_id            TEXT NOT NULL REFERENCES activity_sessions(id),
    sequence              INTEGER NOT NULL,
    source                TEXT NOT NULL CHECK (source IN ('user', 'ai', 'mcp-agent')),
    entry_type            TEXT NOT NULL CHECK (
        entry_type IN (
            'command_proposed', 'decision', 'command_executed', 'ai_message',
            'file_read', 'file_written', 'connection', 'activity_tick'
        )
    ),
    content               BLOB NOT NULL,
    created_at            TEXT NOT NULL,
    action_id             TEXT,
    redaction_incomplete  INTEGER NOT NULL DEFAULT 0,
    rule_snapshot         TEXT,
    UNIQUE (session_id, sequence)
);

INSERT INTO ledger_entries_new (id, session_id, sequence, source, entry_type, content, created_at)
SELECT id, session_id, sequence, source, entry_type, content, created_at FROM ledger_entries;

DROP TABLE ledger_entries;
ALTER TABLE ledger_entries_new RENAME TO ledger_entries;

CREATE INDEX idx_ledger_entries_session ON ledger_entries(session_id, sequence);
CREATE INDEX idx_ledger_entries_created_at ON ledger_entries(created_at);
