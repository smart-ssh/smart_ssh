-- Spec 0057, §2.3 (Etappe 3, rollierende KI-Zusammenfassung): rein additiv
-- — zwei neue, nullable Spalten auf der bestehenden `chat_sessions`-Tabelle
-- (Migration 0008), keine bestehende Spalte/Tabelle wird verändert. Alte
-- Zeilen bekommen für beide neue Spalten automatisch NULL (SQLite-Default
-- für `ALTER TABLE ... ADD COLUMN` ohne `DEFAULT`-Klausel) und bleiben
-- unverändert lesbar — `SqliteChatSessionStore::load_summary` liest das als
-- "noch keine Zusammenfassung vorhanden" (`Ok(None)`), kein Sonderfall
-- nötig.
--
-- `summary_text` ist ein `BLOB` von Anfang an (nonce || ciphertext, s.
-- `ContentCipher::encrypt`/`EncryptedContent::to_blob`) — derselbe
-- Verschlüsselungsmechanismus wie `chat_messages.content` (Spec 0036,
-- Migration 0009) und `ledger_entries.content` (Spec 0057 §1.3, Migration
-- 0011), mit demselben `chat_content_cipher`.
--
-- `summary_rounds_covered` trägt keine vertraulichen Nutzdaten (nur eine
-- Zählung, wie viele der ältesten Gesprächsrunden diese Zusammenfassung
-- bereits abdeckt) und bleibt deshalb bewusst Klartext-`INTEGER`.
--
-- Beide Spalten sind gemeinsam entweder NULL oder gemeinsam gesetzt (von
-- `SqliteChatSessionStore::save_summary` immer paarweise geschrieben) —
-- keine eigene `CHECK`-Constraint dafür, dieselbe Konvention wie
-- `ledger_entries`, wo Anwendungscode (nicht das Schema) diese Invariante
-- durchsetzt.

ALTER TABLE chat_sessions ADD COLUMN summary_text BLOB;
ALTER TABLE chat_sessions ADD COLUMN summary_rounds_covered INTEGER;
