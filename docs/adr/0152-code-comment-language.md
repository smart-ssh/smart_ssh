# ADR 0152 — Code comments in new code are written in English

## Status

Accepted

Betrifft: Issue #872

## Problem

The language of code comments is inconsistent. The only written rule, in the
coder skill, asks for German comments. In practice the older crates are
commented mostly in German (about 80–87 % of comment lines), while the newest
crates (`crates/speech`, `crates/real-app-tests`) are commented entirely in
English. `AGENTS.md`, the changelog and commit messages are English, and the
repository is public.

## Decision

1. Comments in new code are written in English.
2. Existing German comments stay as they are. They may be translated when the
   surrounding function is substantially rewritten, never in bulk.
3. Spec references in comments (`Spec NNNN, A<n>`) stay as they are.

The rule is stated in `AGENTS.md` ("Code comments"); the coder skill points
there instead of carrying its own rule.

## Consequences

- Files are mixed-language during a transition.
- No bulk translation: untouched comments are not changed, and reviewers do
  not ask for translating them.
- Identifiers and log/error text keys are not affected. User-visible text goes
  through i18n as before.
