# Extension observes session activity

## 1. Purpose

An extension needs to be told, as it happens, what a session does (commands,
approval decisions, AI messages, file reads and writes, session start and end),
or the event stream gets a new kind or field. The extension implements
`ActivityObserver` and registers it in `Wiring::activity_observers`; the core
keeps no activity store of its own for this.

## 2. Files to touch

- [ ] `crates/app-logic/src/activity.rs` — the event types, the observer trait and the bounded queue with its worker.
- [ ] `crates/app-logic/src/session/activity_events.rs` — where `Session` builds an event from redacted content.
- [ ] `crates/app-logic/src/orchestration/chat_turn/activity_funnel.rs` — `emit_activity_event`, the one place that redacts and feeds the observers.
- [ ] `crates/app-logic/src/ext.rs` — re-export of new types.
- [ ] `crates/app-shell/src/wiring.rs` — `Wiring::activity_observers`.
- [ ] `crates/app-logic/src/orchestration/action_exec/tests_activity.rs` — a test per event kind.
- [ ] `THREAT-MODEL.md` — paragraph "Activity observers".
- [ ] `docs/codemap.md` — the extension-point entry.

## 3. Required tests

- The event is built from redacted content: plant a secret and assert it is absent from the event.
- With no observer registered no event is constructed.
- A blocking or panicking observer neither delays nor breaks a command.

## 4. Guard that catches a forgotten step

`test_community_registers_no_activity_observers` in `crates/app-shell/src/wiring.rs` keeps the default path unchanged. There is no guard for the threat-model paragraph; the review checks it.

## 5. Example

Issue #1505 (the hook itself).
