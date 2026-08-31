CREATE TABLE filter_rules (
    id            TEXT PRIMARY KEY,
    pattern_type  TEXT NOT NULL CHECK (pattern_type IN ('glob', 'regex', 'exact')),
    pattern_value TEXT NOT NULL,
    action        TEXT NOT NULL CHECK (action IN ('allow', 'confirm', 'deny')),
    scope_type    TEXT NOT NULL CHECK (scope_type IN ('global', 'server', 'tag')),
    scope_value   TEXT,   -- server_id oder Tag-Name, NULL bei 'global'
    priority      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE INDEX idx_filter_rules_scope ON filter_rules(scope_type, scope_value);
