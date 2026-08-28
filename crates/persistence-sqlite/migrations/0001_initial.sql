-- no-transaction
-- sqlx-Direktive (muss exakt so als erste Zeile stehen): SQLite verbietet
-- den Wechsel des Journal-Modus innerhalb einer Transaktion, sqlx wrappt
-- Migrationen aber standardmäßig in eine Transaktion. Ohne dieses Opt-out
-- schlägt "PRAGMA journal_mode = WAL" unten mit "cannot change into wal
-- mode from within a transaction" fehl (reproduzierbar gegen eine echte
-- Datei — bei :memory: bleibt es unbemerkt, da SQLite WAL dort ohnehin
-- ignoriert). Der eigentliche Inhalt ab hier entspricht exakt Abschnitt 4
-- der Spec.

PRAGMA journal_mode = WAL;

CREATE TABLE groups (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    parent_id   TEXT REFERENCES groups(id) ON DELETE CASCADE,
    notes       TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX idx_groups_parent ON groups(parent_id);

CREATE TABLE servers (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    host            TEXT NOT NULL,
    port            INTEGER NOT NULL,
    username        TEXT NOT NULL,
    group_id        TEXT REFERENCES groups(id) ON DELETE SET NULL,
    auth_method     TEXT NOT NULL,   -- JSON-serialisiertes AuthMethod-Enum
    notes           TEXT NOT NULL DEFAULT '',
    jump_host_id    TEXT REFERENCES servers(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_servers_group ON servers(group_id);

CREATE TABLE server_tags (
    server_id   TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    tag         TEXT NOT NULL,
    PRIMARY KEY (server_id, tag)
);

CREATE INDEX idx_server_tags_tag ON server_tags(tag);

CREATE TABLE note_revisions (
    id              TEXT PRIMARY KEY,
    target_type     TEXT NOT NULL CHECK (target_type IN ('server', 'group')),
    target_id       TEXT NOT NULL,
    content         TEXT NOT NULL,
    editor_type     TEXT NOT NULL CHECK (editor_type IN ('user', 'ai')),
    ai_provider     TEXT,
    ai_model        TEXT,
    created_at      TEXT NOT NULL
);

CREATE INDEX idx_note_revisions_target ON note_revisions(target_type, target_id);
