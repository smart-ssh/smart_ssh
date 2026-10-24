-- Spec 0109 I4, Spec 0103: servers and groups that a backup import moved into
-- "Previous data <date>" are marked as copies. Additive; 0 = regular entry.
ALTER TABLE servers ADD COLUMN is_copy INTEGER NOT NULL DEFAULT 0;
ALTER TABLE groups ADD COLUMN is_copy INTEGER NOT NULL DEFAULT 0;
