-- Spec 0036, Abschnitt 3: `chat_messages.content` wird von TEXT auf BLOB
-- umgestellt — gespeichert wird ab jetzt `nonce || ciphertext` (Spec 0036,
-- Abschnitt 3) statt des serialisierten Klartexts. Eigene, neue Migration
-- statt die bestehende `0008_chat_sessions.sql` nachträglich zu ändern
-- (Spec-0036-Aufgabenstellung, Punkt 1) — `0008` bleibt unverändert
-- korrekt für den Stand, den sie ursprünglich eingeführt hat.
--
-- Kein Daten-Migrationsskript für Bestandszeilen nötig (Spec 0036,
-- Abschnitt 5): `chat_sessions`/`chat_messages` wurden in dieser
-- Code-Basis gerade erst mit Spec 0034 eingeführt (derselbe
-- Implementierungsdurchlauf, unmittelbar vor diesem Schritt) und nie mit
-- Klartext-Inhalten ausgeliefert — es gibt keinen produktiven Bestand, den
-- ein Skript verschlüsseln müsste. SQLite kennt kein `ALTER COLUMN ... TYPE`,
-- daher die Standard-Technik: Tabelle neu anlegen, migrieren, umbenennen.

PRAGMA foreign_keys = OFF;

CREATE TABLE chat_messages_new (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'action_result')),
    content_type    TEXT NOT NULL CHECK (
        content_type IN ('text', 'command_result', 'action_rejected', 'document')
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
