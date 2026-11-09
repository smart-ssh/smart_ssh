# ADR 0156 — Interactive terminal holds the wake lock while active

## Status

Accepted

Betrifft: Issue #784, Spec 0114 §4, ADR 0149

## Problem

The partial wake lock was held only for AI-run commands and transfers, so a
long command typed in the interactive terminal could stall when the device
slept. A shell has no "command finished" signal.

## Decision

1. The terminal actor holds an activity guard while input or output
   happened within the last 60 seconds (`TERMINAL_IDLE_TIMEOUT`); every
   input or output chunk extends the deadline.
2. After 60 s without activity, and when the actor ends (session close,
   error, cancellation), the guard is dropped. It shares the process-wide
   counter with AI commands and transfers, so the lock is released only
   when the last holder ends.
3. Wake-lock failures are only logged, as before.

60 s covers typical quiet phases of long commands (compiles, backups) at a
bounded battery cost; holding it for the whole open terminal was rejected as
a battery drain, not holding it as leaving the stall in place.
