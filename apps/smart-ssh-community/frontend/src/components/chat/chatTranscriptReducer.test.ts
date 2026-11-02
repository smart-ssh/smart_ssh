import { describe, expect, it } from "vitest";
import { chatTranscriptReducer, initialChatTranscriptState, type ChatTranscriptAction, type ChatTranscriptState } from "./chatTranscriptReducer";
import type { ChatItem } from "./chatItem";

type ActionItem = Extract<ChatItem, { type: "action" }>;

const run = (state: ChatTranscriptState, ...actions: ChatTranscriptAction[]) =>
  actions.reduce(chatTranscriptReducer, state);

const base = () => initialChatTranscriptState(1000);

const actionItem = (actionId: string, extra: Partial<ActionItem> = {}): ActionItem =>
  ({
    type: "action",
    id: `i-${actionId}`,
    actionId,
    action: { command: "ls" },
    decision: { Confirm: { reason: "r", code: "c" } },
    responded: false,
    riskAssessment: { serverRisk: "green", serverRiskReason: null, dataRisk: "green", dataRiskReason: null, aiReviewed: false },
    riskSecondOpinionPending: true,
    secondOpinionUnavailable: false,
    startedAt: null,
    ...extra,
  }) as unknown as ActionItem;

const user = (id: string, extra: Record<string, unknown> = {}): ChatItem => ({ type: "user", id, text: id, ...extra }) as ChatItem;

const errorAction = (id: string, isRetryableCode: boolean): ChatTranscriptAction => ({
  type: "error",
  id,
  message: "boom",
  code: "X",
  details: {} as never,
  isRetryableCode,
});

describe("chatTranscriptReducer", () => {
  it("starts an AI wait on request start", () => {
    const s = run(base(), { type: "aiRequestStarted", provider: "p", now: 5 });
    expect(s.aiWait).toEqual({ provider: "p", since: 5, gotContent: false });
    expect(s.waitNow).toBe(5);
  });

  it("appends deltas to the last assistant item or starts a new one", () => {
    let s = run(base(), { type: "textDelta", delta: "he", now: 1, newId: "a1" });
    expect(s.items).toEqual([{ type: "assistant", id: "a1", text: "he" }]);
    s = run(s, { type: "textDelta", delta: "llo", now: 2, newId: "a2" });
    expect(s.items).toEqual([{ type: "assistant", id: "a1", text: "hello" }]);
    s = run(s, { type: "responseEmpty", id: "e" }, { type: "textDelta", delta: "x", now: 3, newId: "a3" });
    expect(s.items.map((i) => i.id)).toEqual(["a1", "e", "a3"]);
  });

  it("a delta clears retrying and marks the wait as having content", () => {
    let s = run(base(), { type: "aiRequestStarted", provider: "p", now: 5 }, { type: "aiRetrying", attempt: 1, max: 3, deadline: 9, now: 6 });
    expect(s.retrying).toEqual({ attempt: 1, max: 3, deadline: 9 });
    s = run(s, { type: "textDelta", delta: "x", now: 7, newId: "a" });
    expect(s.retrying).toBeNull();
    expect(s.aiWait).toEqual({ provider: "p", since: 7, gotContent: true });
  });

  it("a delta keeps the object identity of all items except the last", () => {
    const first = user("u1");
    const second = actionItem("a");
    let s = run({ ...base(), items: [first, second] }, { type: "textDelta", delta: "a", now: 1, newId: "x" });
    const before = s.items;
    s = run(s, { type: "textDelta", delta: "b", now: 2, newId: "y" });
    expect(s.items[0]).toBe(before[0]);
    expect(s.items[1]).toBe(before[1]);
    expect(s.items[2]).not.toBe(before[2]);
    expect(s.items).toHaveLength(3);
  });

  it("an action proposal ends the AI wait state", () => {
    const s = run(
      base(),
      { type: "aiRequestStarted", provider: "p", now: 1 },
      { type: "aiRetrying", attempt: 1, max: 2, deadline: 3, now: 1 },
      { type: "actionProposed", item: actionItem("a") },
    );
    expect(s.aiWait).toBeNull();
    expect(s.retrying).toBeNull();
    expect(s.items).toHaveLength(1);
  });

  it("a risk assessment update changes only the matching action item", () => {
    const a = actionItem("a");
    const b = actionItem("b");
    const u = user("u");
    const s = run({ ...base(), items: [u, a, b] }, {
      type: "riskAssessmentUpdated",
      actionId: "a",
      dataRisk: "red",
      reason: "why",
      secondOpinionUnavailable: true,
    });
    expect(s.items[0]).toBe(u);
    expect(s.items[2]).toBe(b);
    const changed = s.items[1] as ActionItem;
    expect(changed.riskAssessment?.dataRisk).toBe("red");
    expect(changed.riskAssessment?.dataRiskReason).toBe("why");
    expect(changed.riskAssessment?.aiReviewed).toBe(true);
    expect(changed.riskSecondOpinionPending).toBe(false);
    expect(changed.secondOpinionUnavailable).toBe(true);
  });

  it("an escalated decision becomes Confirm and resets startedAt", () => {
    const s = run({ ...base(), items: [actionItem("a", { decision: "AutoExec", startedAt: 5 })] }, {
      type: "decisionEscalated",
      actionId: "a",
      reason: "r2",
      code: "c2",
    });
    const it = s.items[0] as ActionItem;
    expect(it.decision).toEqual({ Confirm: { reason: "r2", code: "c2" } });
    expect(it.startedAt).toBeNull();
  });

  it("tracks live output and waiting input, and the result replaces them", () => {
    let s = run(
      base(),
      { type: "actionOutput", actionId: "a", event: { sessionId: "s", actionId: "a", stream: "stdout", chunk: "hi" } as never },
      { type: "actionWaitingInput", actionId: "a", waiting: true },
    );
    expect(Object.keys(s.liveOutputs)).toEqual(["a"]);
    expect(s.waitingActions).toEqual({ a: true });
    s = run({ ...s, items: [actionItem("a")] }, { type: "actionResult", actionId: "a", result: { exitCode: 0 } as never });
    expect(s.liveOutputs).toEqual({});
    expect(s.waitingActions).toEqual({});
    expect((s.items[0] as ActionItem).responded).toBe(true);
    expect((s.items[0] as ActionItem).result).toEqual({ exitCode: 0 });
    s = run(s, { type: "actionWaitingInput", actionId: "zzz", waiting: false });
    expect(s.waitingActions).toEqual({});
  });

  it("marks a settled confirmation", () => {
    const s = run({ ...base(), items: [actionItem("a")] }, { type: "confirmationSettled", actionId: "a", reason: "timeout" });
    expect(s.items[0]).toMatchObject({ responded: true, settled: "timeout" });
  });

  it("an error after only a user message is retryable; after a reply it is not", () => {
    let s = run({ ...base(), items: [user("u")] }, errorAction("e1", true));
    expect(s.items[1]).toMatchObject({ type: "error", retryable: true, code: "X" });
    s = run({ ...base(), items: [user("u"), { type: "assistant", id: "a", text: "x" }] }, errorAction("e2", true));
    expect(s.items[2]).toMatchObject({ retryable: false });
    s = run({ ...base(), items: [user("u")] }, errorAction("e3", false));
    expect(s.items[1]).toMatchObject({ retryable: false });
    s = run({ ...base(), items: [] }, errorAction("e4", true));
    expect(s.items[0]).toMatchObject({ retryable: false });
  });

  it("an error clears retry and wait state", () => {
    const s = run(
      base(),
      { type: "aiRequestStarted", provider: "p", now: 1 },
      { type: "aiRetrying", attempt: 1, max: 2, deadline: 3, now: 1 },
      errorAction("e", false),
    );
    expect(s.aiWait).toBeNull();
    expect(s.retrying).toBeNull();
  });

  it("a truncated response marks the last assistant item or adds an empty one", () => {
    let s = run(base(), { type: "textDelta", delta: "x", now: 1, newId: "a" }, { type: "responseTruncated", newId: "n" });
    expect(s.items).toEqual([{ type: "assistant", id: "a", text: "x", truncated: true }]);
    s = run({ ...base(), items: [user("u")] }, { type: "responseTruncated", newId: "n" });
    expect(s.items[1]).toEqual({ type: "assistant", id: "n", text: "", truncated: true });
  });

  it("appends empty / refused / unavailable / compacted / cancelled / limit items", () => {
    const s = run(
      base(),
      { type: "responseEmpty", id: "1" },
      { type: "responseRefused", id: "2" },
      { type: "webResearchUnavailable", id: "3" },
      { type: "contextCompacted", id: "4" },
      { type: "autoContinuationLimitReached", id: "5", limit: 7 },
      { type: "webActivity", id: "6", activity: {} as never, activityId: "w" },
    );
    expect(s.items.map((i) => i.type)).toEqual([
      "emptyResponse",
      "refusedResponse",
      "webResearchUnavailable",
      "contextCompacted",
      "autoContinuationLimitReached",
      "webActivity",
    ]);
  });

  it("a cancelled response clears retrying", () => {
    const s = run(base(), { type: "aiRetrying", attempt: 1, max: 2, deadline: 3, now: 1 }, { type: "responseCancelled", id: "c" });
    expect(s.retrying).toBeNull();
    expect(s.items[0].type).toBe("responseCancelled");
  });

  it("queued messages sent clears only the queued flag", () => {
    const keep = user("k");
    const s = run({ ...base(), items: [user("q", { queued: true }), keep] }, { type: "queuedMessagesSent" });
    expect(s.items[0]).toMatchObject({ queued: false });
    expect(s.items[1]).toBe(keep);
  });

  it("stores context usage and the injection check", () => {
    const s = run(
      base(),
      { type: "contextUsage", inputTokens: 5, windowTokens: 10 },
      { type: "injectionCheck", itemId: "i", state: { status: "clean" } as never },
    );
    expect(s.contextUsage).toEqual({ inputTokens: 5, windowTokens: 10 });
    expect(s.injectionChecks).toEqual({ i: { status: "clean" } });
  });

  it("auto continuation starts, stops and settles", () => {
    let s = run(base(), { type: "autoContinuationStarted" });
    expect(s.autoContinuing).toBe(true);
    s = run(s, { type: "aiRequestStarted", provider: "p", now: 1 }, { type: "autoContinuationStopped" });
    expect(s.autoContinuing).toBe(false);
    expect(s.aiWait).toBeNull();
    s = run({ ...base(), items: [user("q", { queued: true })], autoContinuing: true }, { type: "turnSettled" });
    expect(s.autoContinuing).toBe(false);
    expect(s.items[0]).toMatchObject({ queued: false });
  });

  it("budget waiting, question asked and document generated append cards; the card can be removed", () => {
    let s = run(
      base(),
      { type: "aiBudgetWaiting", id: "b", waitSeconds: 3 },
      { type: "questionAsked", id: "q", questionId: "qq", question: "?", options: [], multiSelect: false },
      { type: "documentGenerated", id: "d", title: "T", contentMarkdown: "# x" },
    );
    expect(s.items.map((i) => i.type)).toEqual(["aiBudgetWaiting", "question", "document"]);
    expect(s.items[1]).toMatchObject({ questionId: "qq", multiSelect: false });
    expect(s.items[2]).toMatchObject({ title: "T", contentMarkdown: "# x" });
    s = run(s, { type: "itemRemoved", id: "b" });
    expect(s.items.map((i) => i.id)).toEqual(["q", "d"]);
  });

  it("ticks update the clocks", () => {
    const s = run(base(), { type: "waitTick", now: 50 }, { type: "retryTick", now: 60 });
    expect([s.waitNow, s.retryNow]).toEqual([50, 60]);
  });

  it("history is prepended; an empty history changes nothing", () => {
    const s0 = { ...base(), items: [user("live")] };
    expect(run(s0, { type: "historyLoaded", entries: [] })).toBe(s0);
    const s = run(s0, { type: "historyLoaded", entries: [user("old")] });
    expect(s.items.map((i) => i.id)).toEqual(["old", "live"]);
  });

  it("local actions: respond, rollback, rule accepting, truncated cleared, chat cleared", () => {
    let s = run({ ...base(), items: [actionItem("a")] }, { type: "actionResponded", actionId: "a", decision: "approve", startedAt: 9 });
    expect(s.items[0]).toMatchObject({ responded: true, userDecision: "approve", startedAt: 9 });
    s = run(s, { type: "actionRespondFailed", actionId: "a", unknown: false, errorItem: user("err") });
    expect(s.items[0]).toMatchObject({ responded: false, startedAt: null });
    expect(s.items[1].id).toBe("err");
    s = run(s, { type: "actionRespondFailed", actionId: "a", unknown: true, errorItem: user("err2") });
    expect(s.items[0]).toMatchObject({ responded: true, settled: "unknown" });
    s = run(s, { type: "ruleAccepting", actionId: "a", startedAt: 4 });
    expect(s.items[0]).toMatchObject({ startedAt: 4 });
    s = run({ ...s, items: [{ type: "assistant", id: "x", text: "t", truncated: true }] }, { type: "truncatedCleared", id: "x" });
    expect(s.items[0]).toMatchObject({ truncated: false });
    s = run({ ...s, contextUsage: { inputTokens: 5, windowTokens: 10 } }, { type: "chatCleared" });
    expect(s.items).toEqual([]);
    expect(s.contextUsage).toEqual({ inputTokens: 0, windowTokens: 10 });
  });

  it("a resync merges the rebuilt items", () => {
    const s = run({ ...base(), items: [user("u")] }, { type: "resynced", rebuilt: [user("u2")], pendingActionId: null });
    expect(s.items.length).toBeGreaterThan(0);
  });

  it("a turn start drops a stale wait line", () => {
    const s = run(base(), { type: "aiRequestStarted", provider: "p", now: 1 }, { type: "turnStarted" });
    expect(s.aiWait).toBeNull();
  });
});
