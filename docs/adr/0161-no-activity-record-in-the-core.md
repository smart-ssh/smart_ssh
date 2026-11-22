# ADR 0161 — No activity record in the core

## Status

Accepted · Issue: #1506 · Epic #1442 · Spec 0057 · Supersedes ADR 0047 and ADR 0160

## Context

The epic first built a persistent activity record inside the core: a ledger table, "recorded sessions" with server snapshots, a read API and retention. The activity observer hook (#1505) later delivers the same redacted events to extensions. Keeping both gives two sources of truth, a second copy of command output on disk, and a read API that every edition would have to maintain.

## Decisions

| Question | Decision | Why |
| --- | --- | --- |
| Does the core keep an activity record? | No. Events go only to registered observers. | One redaction point, one delivery path; storing and presenting history is an extension's job. |
| What happens to existing ledger data? | Migration 0030 drops `ledger_entries`, `activity_sessions` and the `chat_sessions.activity_session_id` column. Chats, messages, summaries and notes are untouched. | The data was never shown anywhere in the core; keeping an unreadable table would only hold old output on disk. Removal is intentional and irreversible. |
| What does the redaction funnel do now? | `emit_activity_event` redacts once and feeds the observers; nothing is persisted. Without an observer nothing is built. | Same redaction guarantee as before, no write path left. |
| Type names | The `core::audit` content types (`LedgerEntryContent`, `LedgerSource`, …) keep their names as the content model of the events. | A rename across the workspace would add churn without behaviour change. |
| Enforcement | A guard test fails when non-test code references the removed tables or stores. | Prevents the record from creeping back. |
| ADR 0047 and 0160 | Marked superseded. | Their ledger scope and file-action entries now describe event content only (Spec 0057 §1.1). |
