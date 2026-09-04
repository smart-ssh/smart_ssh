-- Spec 0040, Abschnitt 3 (löst Spec 0036 §6): `prompt_history.content`
-- wird von TEXT auf BLOB umgestellt, verschlüsselt über denselben
-- `ContentCipher`/Schlüssel wie `chat_messages` (Spec 0036) — kein zweiter
-- Verschlüsselungsmechanismus.
--
-- Anders als bei `chat_messages` (Migration 0009) kann `prompt_history`
-- bereits echte Klartext-Bestandsdaten enthalten: die Tabelle existiert
-- seit Spec 0015 und die App hat laut Architektur-Brief bereits
-- Downloads über die Marketing-Website. Diese Migration stellt nur das
-- SCHEMA um (SQLite kennt kein `ALTER COLUMN ... TYPE`, daher Tabelle neu
-- anlegen) — die vorhandenen Bytes werden unverändert in die neue
-- BLOB-Spalte übernommen (bei einer TEXT-Spalte sind das exakt die
-- UTF-8-Bytes des bisherigen Klartexts). Das eigentliche Verschlüsseln
-- bestehender Klartext-Zeilen übernimmt NICHT diese SQL-Migration
-- (SQLite kann keine Kryptografie), sondern die anwendungsseitige,
-- idempotente Routine `SqlitePromptHistoryStore::migrate_legacy_plaintext_
-- content`, die beim App-Start läuft (s. dortiger Kommentar) — sie läuft
-- nach dieser Migration und erkennt bereits verschlüsselte Zeilen daran,
-- dass sie sich mit dem aktuellen Schlüssel entschlüsseln lassen.

PRAGMA foreign_keys = OFF;

CREATE TABLE prompt_history_new (
    id          TEXT PRIMARY KEY,
    server_id   TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    content     BLOB NOT NULL,   -- nonce || ciphertext (Spec 0036, Abschnitt 3), ggf. noch unverschlüsselt bis zur Anwendungs-Migration
    created_at  TEXT NOT NULL
);

INSERT INTO prompt_history_new (id, server_id, content, created_at)
SELECT id, server_id, content, created_at FROM prompt_history;

DROP TABLE prompt_history;
ALTER TABLE prompt_history_new RENAME TO prompt_history;

CREATE INDEX idx_prompt_history_server ON prompt_history(server_id, created_at);

PRAGMA foreign_keys = ON;
