CREATE TABLE prompt_history (
    id          TEXT PRIMARY KEY,
    server_id   TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE INDEX idx_prompt_history_server ON prompt_history(server_id, created_at);
