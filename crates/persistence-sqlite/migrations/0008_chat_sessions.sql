-- Spec 0034, Abschnitt 2: persistente, fortsetzbare KI-Chat-Sitzungen.

CREATE TABLE chat_sessions (
    id              TEXT PRIMARY KEY,
    server_id       TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    title           TEXT,             -- NULL bis automatisch generiert
    started_at      TEXT NOT NULL,
    ended_at        TEXT,             -- NULL während aktiv/laufend
    ai_provider_id  TEXT REFERENCES ai_provider_configs(id) ON DELETE SET NULL
);

CREATE INDEX idx_chat_sessions_server ON chat_sessions(server_id, started_at);

CREATE TABLE chat_messages (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'action_result')),
    content_type    TEXT NOT NULL CHECK (
        content_type IN ('text', 'command_result', 'action_rejected', 'document')
    ),
    content         TEXT NOT NULL,    -- serialisierter MessageContent (JSON), siehe Abschnitt 3
    sequence        INTEGER NOT NULL, -- Reihenfolge innerhalb der Sitzung
    created_at      TEXT NOT NULL
);

CREATE INDEX idx_chat_messages_session ON chat_messages(session_id, sequence);
