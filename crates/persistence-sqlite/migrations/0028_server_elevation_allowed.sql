-- Issue #1194, Spec 0067 A5: per-server switch for elevated rights in the
-- file browser. Additive; 1 = allowed (existing servers keep the behaviour).
ALTER TABLE servers ADD COLUMN elevation_allowed INTEGER NOT NULL DEFAULT 1;
