-- Issue #903: `(session_id, sequence)` is unique per chat session. Two
-- concurrent appends could previously both read the same MAX(sequence) and
-- store duplicates. Earlier migrations stay byte-identical (sqlx checks
-- checksums).
--
-- Step 1: renumber every session that has duplicate pairs, keeping the
-- relative order (sequence, created_at, id) and starting at 0. Sessions
-- without duplicates are left untouched.
-- The new numbers are materialised first: a correlated subquery inside the
-- UPDATE would see rows that are already renumbered.
CREATE TEMP TABLE chat_messages_renumber AS
SELECT id, ROW_NUMBER() OVER (
    PARTITION BY session_id ORDER BY sequence, created_at, id
) - 1 AS new_sequence
FROM chat_messages
WHERE session_id IN (
    SELECT session_id FROM chat_messages
    GROUP BY session_id, sequence HAVING COUNT(*) > 1
);

UPDATE chat_messages
SET sequence = (
    SELECT new_sequence FROM chat_messages_renumber
    WHERE chat_messages_renumber.id = chat_messages.id
)
WHERE id IN (SELECT id FROM chat_messages_renumber);

DROP TABLE chat_messages_renumber;

-- Step 2: replace the non-unique index by a unique one.
DROP INDEX idx_chat_messages_session;
CREATE UNIQUE INDEX idx_chat_messages_session ON chat_messages(session_id, sequence);
