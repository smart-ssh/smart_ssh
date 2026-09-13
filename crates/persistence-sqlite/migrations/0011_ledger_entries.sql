-- Spec 0057, §1 (Ledger-Grundgerüst) + §5 (Migration): rein additiv — eine
-- neue Tabelle, keine bestehende Tabelle wird verändert. Alte Sessions
-- bleiben unberührt und lesbar (Spec 0057, §5: "Keine Daten-Migration").
--
-- Referenziert `chat_sessions(id)` (Migration 0008) genauso wie
-- `chat_messages` — dieselbe `ON DELETE CASCADE`-Regel: löscht der Nutzer
-- eine Sitzung, verschwindet ihr Ledger automatisch mit, kein separater
-- Aufräum-Code nötig.
--
-- `content` ist ein `BLOB` von Anfang an (nicht wie `chat_messages.content`
-- zunächst `TEXT`, erst später auf `BLOB` migriert, Migration 0009) — die
-- Verschlüsselung (Spec 0057, §1.3/Spec 0036-Muster) ist von Tag eins an
-- vorgesehen, es gibt keine unverschlüsselte Vorgänger-Fassung dieser
-- Tabelle, die nachträglich umgestellt werden müsste.

CREATE TABLE ledger_entries (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    -- Append-only, monoton pro Sitzung (Spec 0057, §1: "Reihenfolge-
    -- garantiert") — dieselbe "MAX(sequence) + 1"-Vergabe wie
    -- `chat_messages.sequence` (Migration 0008), kein eigener Zähler nötig.
    sequence        INTEGER NOT NULL,
    -- Spec 0057, §1.1: "Quelle (user/ai/mcp-agent) — für spätere
    -- Audit-„wer"-Unterscheidung".
    source          TEXT NOT NULL CHECK (source IN ('user', 'ai', 'mcp-agent')),
    -- Spiegelt `ssh_manager_core::audit::LedgerEntryContent`s Varianten —
    -- s. dortigen Doc-Kommentar für die genaue Bedeutung jedes Werts.
    entry_type      TEXT NOT NULL CHECK (
        entry_type IN (
            'command_proposed', 'decision', 'command_executed', 'ai_message'
        )
    ),
    -- nonce || ciphertext (Spec 0036-Muster, hier auf den Ledger
    -- ausgeweitet gemäß Spec 0057, §1.3) — serialisiertes, bereits
    -- redigiertes `LedgerEntryContent`-JSON als Klartext vor der
    -- Verschlüsselung.
    content         BLOB NOT NULL,
    created_at      TEXT NOT NULL,
    -- spec-reviewer-Fund (Review dieses Schritts): "MAX(sequence)+1" +
    -- `INSERT` ist nicht atomar — ohne diese Constraint könnten zwei
    -- gleichzeitige Appends derselben Sitzung (Chat-Turn + MCP-Aktion,
    -- Spec 0040: laufen nachweislich parallel) dieselbe `sequence`
    -- bekommen und eine in einem append-only Audit-Protokoll unzulässige
    -- undefinierte Reihenfolge erzeugen. Ein Verstoß schlägt so als
    -- Insert-Fehler fehl (von `SqliteLedgerStore::append_entry` nur
    -- geloggt, nicht fatal, s. dortiger Doc-Kommentar) statt eine
    -- Kollision still zu verschlucken.
    UNIQUE (session_id, sequence)
);

CREATE INDEX idx_ledger_entries_session ON ledger_entries(session_id, sequence);
