-- Spec 0067, A2: optionaler Override für den Pfad von `sftp-server` im
-- erhöhten Dateibrowser-Modus. NULL = automatisch erkennen.
ALTER TABLE servers ADD COLUMN sftp_server_path TEXT;
