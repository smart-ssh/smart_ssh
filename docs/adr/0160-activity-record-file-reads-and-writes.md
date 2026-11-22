# ADR 0160 — Activity record: AI file reads and writes

## Status

Accepted · Issue: #1446 · Spec 0057 · Supersedes ADR 0047, point 3 (for file actions)

## Context

ADR 0047 point 3 limited the ledger to `SuggestCommand`. A file read or write by the AI left no entry; a write left only a chat line with the backup path. The activity record (epic #1442) must show which files the AI touched.

## Decisions

| Question | Decision | Why |
| --- | --- | --- |
| What is recorded for a read? | `file_read`: path, size in bytes, SHA-256 of the bytes read. | Enough to tell which version of a file the AI saw, without keeping the content. |
| What is recorded for a write? | `file_written`: path, backup path or none, size and SHA-256 before and after, whether the sudo fallback was used. | Lets a reader tie a change to the backup and verify it later. |
| Content or diff? | Never stored. | The record is not a copy of server data; the hashes suffice. |
| When is `file_written` written? | After the write succeeded, or with `status: failed` when it failed after it started (issue #1485). A rejected or blocked write, or one that failed before the target was touched (permission denied, backup or temp-file failure), leaves no entry. | A mid-stream error can leave the target partly overwritten; the record must say so. "Started" means: the direct SFTP write to the target was issued and failed with anything but permission denied (refused on open), or the privileged `install` exited non-zero. A failed `install` also covers a wrong sudo password, which cannot be told apart from a copy error; the entry therefore says "may be changed". For an existing file a wrong sudo password fails the privileged backup `cp` first, so no entry results. For a new file, after any such failure the target is checked once more: if the stat says it does not exist, nothing was touched and no entry is written; if the stat itself fails with a closed session or permission denied, the state is unknown and the failed entry is written. The shape is a `status` field on `FileWritten` (default `completed` for old rows), not a new entry kind, so readers keep one kind. A failed entry has no after-size or hash and no content. |
| "Before" values | Size from the stat before the write; hash from the content read for the backup. On the sudo path the old content is read over SFTP once more for the hash; it is absent only when that read itself is denied or fails (size still recorded). A new file has no "before". | No privileged read is added; the extra read on the sudo path is an ordinary SFTP read. |
| "After" values | Computed over the bytes sent to the server (after the edition's outgoing transform). | These are the bytes that were written. |
| Keep the hashes? | Yes. The hashes stay, although the SHA-256 of very short or low-entropy content can be guessed by brute force. | They are needed to prove what changed and which version the AI saw. The protection for them is the encrypted database, not the hash; the threat model says so. |
| Redaction | Path and backup path go through the same central redaction as command text. | Spec 0057 §1.2. |
| Link to the action | Entries carry the `action_id` of the proposed action. | Spec 0057 §1. |
| Decision entries for file actions | File writes get confirmed, rejected, auto-approved or blocked decision entries, linked by `action_id` to the `file_written` entry. File reads get none. | Reads need no confirmation; a write's decision is part of the record of who allowed it. |

## Consequences

The negative test for "a read writes no entry" is replaced by positive tests. `FileRead`/`FileWritten` gained fields; no stored row used them before.
