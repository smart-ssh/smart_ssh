-- Exakt Abschnitt 8.1 aus docs/specs/0007-tauri-app-mvp.md. Anders als
-- 0001_initial.sql keine "-- no-transaction"-Direktive nötig: CREATE
-- TABLE/CREATE INDEX sind in SQLite innerhalb einer Transaktion erlaubt
-- (nur ein PRAGMA-Journal-Modus-Wechsel wie in 0001 ist es nicht).

CREATE TABLE ai_provider_configs (
    id                          TEXT PRIMARY KEY,
    provider_type               TEXT NOT NULL CHECK (
        provider_type IN ('openai', 'anthropic', 'generic_openai_compatible', 'ollama')
    ),
    display_name                TEXT NOT NULL,
    base_url                    TEXT,             -- nur für generic/ollama relevant
    model                       TEXT NOT NULL,
    supports_native_tool_calling BOOLEAN NOT NULL DEFAULT TRUE,
    credential_ref              TEXT NOT NULL,    -- Schlüssel in den CredentialStore
    is_active                   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at                  TEXT NOT NULL,
    updated_at                  TEXT NOT NULL
);

-- höchstens ein aktiver Provider gleichzeitig (MVP-Annahme, s. Spec 0006 Abschnitt 8)
CREATE UNIQUE INDEX idx_ai_provider_single_active
    ON ai_provider_configs(is_active) WHERE is_active = TRUE;
