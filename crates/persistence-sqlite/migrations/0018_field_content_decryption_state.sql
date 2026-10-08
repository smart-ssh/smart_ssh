-- Issue #113 (ADR 0112): Rückbau der feldweisen Verschlüsselung (Spec 0036).
--
-- `chat_messages.content`, `ledger_entries.content`, `prompt_history.content`
-- und `chat_sessions.summary_text` trugen bisher je Zeile `nonce ||
-- ciphertext` (ChaCha20-Poly1305 unter dem Wurzelschlüssel K). Seit die
-- ganze Datei verschlüsselt ist (Spec 0101), schreiben die Stores Klartext
-- (als SQLite-TEXT in die bestehenden Spalten, ohne Tabellen-Umbau, s. ADR).
--
-- Warum eine Tabelle und keine SQL-Migration für die Umstellung selbst:
-- Entschlüsseln kann SQL nicht. Die Anwendung entschlüsselt jede Altzeile
-- beim Start mit K und schreibt Klartext zurück — in **einer** Transaktion,
-- die auch diesen Zustand auf `done` setzt. Ein Abbruch hinterlässt also
-- entweder den alten oder den neuen Stand, und der nächste Start fängt neu
-- an. Dieselbe Form wie `secret_migration_state` (Migration 0016).

CREATE TABLE field_content_decryption_state (
    -- Genau eine Zeile, erzwungen vom CHECK.
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    -- `open` — Altzeilen können noch vorhanden sein.
    -- `done` — keine der vier Spalten enthält mehr ein feldweise
    --          verschlüsseltes Chiffrat.
    state TEXT NOT NULL CHECK (state IN ('open', 'done'))
);

-- Auch eine frische Installation beginnt mit `open` und durchläuft die
-- Umstellung einmal (ohne etwas zu finden) — derselbe Weg wie bei einer
-- bestehenden Installation, kein ungetesteter zweiter Startpfad.
INSERT INTO field_content_decryption_state (id, state) VALUES (1, 'open');
