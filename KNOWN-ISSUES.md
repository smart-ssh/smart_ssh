# Known issues

This file lists limitations Smart SSH has today, either because they were
deferred or because they are deliberate. Each entry says what happens, why,
and what you can do about it.

Accepted *security* risks are not repeated here. They are listed in
[`THREAT-MODEL.md`, section 6 ("Known residual risks")](THREAT-MODEL.md#6-known-residual-risks).

If you run into a limitation that is not listed, please report it through the
[issue forms](https://github.com/smart-ssh/smart_ssh/issues/new/choose).

## Limitations

### Long chats are condensed before they are sent to the AI

- **What happens:** When a chat grows large, the context sent to the AI is
  reduced before each request. Old rounds are replaced by a short summary. If
  the summary request fails, the oldest rounds are cut without a summary and
  the AI sees a notice that older conversation was shortened. Details of
  earlier rounds, including which commands were already run, can then be
  missing from what the AI knows. The stored chat history is not changed.
- **Why:** The AI's context window is finite, and a summary is lossy.
  Keeping a session from stalling takes priority over keeping every detail
  ([Spec 0057](docs/specs/0057-rich-session-model.md), section 2).
- **Workaround:** Start a new chat ("New chat") for a new task, or restate
  the facts the AI needs.
- **Status:** by design

### The rule test panel does not show where a matching rule comes from

- **What happens:** The rule test panel shows the decision and the matching
  rule, but not the origin of that rule (for example a built-in rule or a
  rule from another source).
- **Why:** The explained evaluation is meant to name the origin
  ([Spec 0037](docs/specs/0037-entitlements-editionen.md), section 5), but
  the interface does not show it yet. All rules you create yourself have the
  same origin, so you do not miss any information today.
- **Workaround:** none needed while you only use your own rules.
- **Status:** planned

## By design

### The local pseudo-server chat is not saved

- **What happens:** The chat with the local pseudo-server is gone after you
  close it. It does not appear in the list of earlier chats.
- **Why:** Saved chats are bound to a server entry, and the local
  pseudo-server has none
  ([Spec 0034](docs/specs/0034-chat-session-persistence.md), section 12).
- **Workaround:** none.
- **Status:** by design

### Actions from external MCP clients leave no chat history

- **What happens:** Actions requested by an external MCP client appear in
  their own tab, but they are not stored as a chat, get no automatic title,
  and do not trigger a follow-up answer from the built-in AI. They are also
  not part of any summary.
- **Why:** The external client controls its own conversation. Smart SSH only
  shows the confirmation card and returns the result to that client
  ([Spec 0028](docs/specs/0028-mcp-server-integration.md), section 3,
  [Spec 0034](docs/specs/0034-chat-session-persistence.md), section 10,
  [Spec 0104](docs/specs/0104-mcp-sessions.md)).
- **Workaround:** none.
- **Status:** by design
