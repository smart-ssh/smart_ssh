-- Spec 0102: optionales Startverzeichnis für Terminal und Dateibrowser.
-- NULL = nicht gesetzt (Home des Login-Nutzers wie bisher).
ALTER TABLE servers ADD COLUMN start_directory TEXT;
