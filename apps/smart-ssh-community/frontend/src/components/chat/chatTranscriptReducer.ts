import type { ActionResultPayload, ActionUserDecision, InjectionCheckState, QuestionOption, WebActivityDto } from "../../types";
import { appendLiveOutput, type LiveOutput } from "../../liveOutput";
import { mergeResyncedItems } from "../../chatResync";
import type { ChatItem } from "./chatItem";

/** Everything the chat event subscription writes. Pure data, no timers. */
export interface ChatTranscriptState {
  items: ChatItem[];
  /** Issue #601: last real input size reported by the provider. */
  contextUsage: { inputTokens: number; windowTokens: number } | null;
  /** Issue #600: provider overloaded, automatic retry pending. */
  retrying: { attempt: number; max: number; deadline: number } | null;
  retryNow: number;
  /** Issue #694: a request is out; `since` is the last sign of life. */
  aiWait: { provider: string; since: number; gotContent: boolean } | null;
  waitNow: number;
  /** Spec 0021, Abschnitt 5: an automatic follow-up round is running. */
  autoContinuing: boolean;
  /** Issue #325: live output per running action (kept apart from `items`). */
  liveOutputs: Record<string, LiveOutput>;
  /** Issue #693: injection check state per checked item. */
  injectionChecks: Record<string, InjectionCheckState>;
  /** Issue #327: actions whose command is probably waiting for input. */
  waitingActions: Record<string, true>;
}

export function initialChatTranscriptState(now: number = Date.now()): ChatTranscriptState {
  return {
    items: [],
    contextUsage: null,
    retrying: null,
    retryNow: now,
    aiWait: null,
    waitNow: now,
    autoContinuing: false,
    liveOutputs: {},
    injectionChecks: {},
    waitingActions: {},
  };
}

type ActionItem = Extract<ChatItem, { type: "action" }>;
type ErrorDetails = NonNullable<Extract<ChatItem, { type: "error" }>["details"]>;

/**
 * One action per chat event, plus the local actions the panel dispatches.
 * Anything impure (ids, clock) is carried by the action, never read here.
 */
export type ChatTranscriptAction =
  // --- events ---
  | { type: "aiRequestStarted"; provider: string; now: number }
  | { type: "textDelta"; delta: string; now: number; newId: string }
  | { type: "actionProposed"; item: ChatItem }
  | {
      type: "riskAssessmentUpdated";
      actionId: string;
      dataRisk: NonNullable<Extract<ChatItem, { type: "action" }>["riskAssessment"]>["dataRisk"];
      reason: string | null;
      secondOpinionUnavailable?: boolean;
    }
  | { type: "decisionEscalated"; actionId: string; reason: string; code: string }
  | { type: "actionOutput"; actionId: string; event: Parameters<typeof appendLiveOutput>[1] }
  | { type: "actionWaitingInput"; actionId: string; waiting: boolean }
  | { type: "confirmationSettled"; actionId: string; reason: ActionItem["settled"] }
  | { type: "actionResult"; actionId: string; result: ActionResultPayload }
  | { type: "error"; id: string; message: string; code?: string | null; details: ErrorDetails; retryable?: boolean; isRetryableCode: boolean }
  | { type: "aiRetrying"; attempt: number; max: number; deadline: number; now: number }
  | { type: "responseTruncated"; newId: string }
  | { type: "injectionCheck"; itemId: string; state: InjectionCheckState }
  | { type: "webActivity"; id: string; activity: WebActivityDto; activityId: string | undefined }
  | { type: "responseEmpty"; id: string }
  | { type: "responseRefused"; id: string }
  | { type: "webResearchUnavailable"; id: string }
  | { type: "queuedMessagesSent" }
  | { type: "contextUsage"; inputTokens: number; windowTokens: number }
  | { type: "contextCompacted"; id: string }
  | { type: "responseCancelled"; id: string }
  | { type: "autoContinuationLimitReached"; id: string; limit: number }
  | { type: "autoContinuationStarted" }
  | { type: "aiBudgetWaiting"; id: string; waitSeconds: number }
  | { type: "questionAsked"; id: string; questionId: string; question: string; options: QuestionOption[]; multiSelect: boolean }
  | { type: "documentGenerated"; id: string; title: string; contentMarkdown: string }
  // --- timers (owned by the hook) ---
  | { type: "waitTick"; now: number }
  | { type: "retryTick"; now: number }
  // --- local actions ---
  | { type: "historyLoaded"; entries: ChatItem[] }
  | { type: "resynced"; rebuilt: ChatItem[]; pendingActionId: string | null }
  | { type: "itemAppended"; item: ChatItem }
  | { type: "itemRemoved"; id: string }
  | { type: "truncatedCleared"; id: string }
  | { type: "actionResponded"; actionId: string; decision: ActionUserDecision["decision"]; startedAt: number | null }
  | { type: "actionRespondFailed"; actionId: string; unknown: boolean; errorItem: ChatItem }
  | { type: "ruleAccepting"; actionId: string; startedAt: number }
  | { type: "autoContinuationStopped" }
  | { type: "turnStarted" }
  | { type: "turnSettled" }
  | { type: "chatCleared" };

const mapAction = (
  items: ChatItem[],
  actionId: string,
  update: (item: ActionItem) => ActionItem,
): ChatItem[] =>
  items.map((item) => (item.type === "action" && item.actionId === actionId ? update(item) : item));

const withoutKey = <T,>(rec: Record<string, T>, key: string): Record<string, T> => {
  if (!(key in rec)) return rec;
  const { [key]: _removed, ...rest } = rec;
  return rest;
};

const clearQueued = (items: ChatItem[]): ChatItem[] =>
  items.map((it) => (it.type === "user" && it.queued ? { ...it, queued: false } : it));

const append = (state: ChatTranscriptState, item: ChatItem): ChatTranscriptState => ({
  ...state,
  items: [...state.items, item],
});

export function chatTranscriptReducer(
  state: ChatTranscriptState,
  action: ChatTranscriptAction,
): ChatTranscriptState {
  switch (action.type) {
    case "aiRequestStarted":
      return {
        ...state,
        waitNow: action.now,
        aiWait: { provider: action.provider, since: action.now, gotContent: false },
      };
    case "textDelta": {
      const last = state.items[state.items.length - 1];
      const items =
        last?.type === "assistant"
          ? [...state.items.slice(0, -1), { ...last, text: last.text + action.delta }]
          : [...state.items, { type: "assistant" as const, id: action.newId, text: action.delta }];
      return {
        ...state,
        retrying: null,
        aiWait: state.aiWait ? { ...state.aiWait, since: action.now, gotContent: true } : state.aiWait,
        items,
      };
    }
    case "actionProposed":
      return { ...append(state, action.item), retrying: null, aiWait: null };
    case "riskAssessmentUpdated":
      return {
        ...state,
        items: state.items.map((item) =>
          item.type === "action" && item.actionId === action.actionId && item.riskAssessment
            ? {
                ...item,
                riskAssessment: {
                  ...item.riskAssessment,
                  dataRisk: action.dataRisk,
                  dataRiskReason: action.reason,
                  aiReviewed: true,
                },
                riskSecondOpinionPending: false,
                secondOpinionUnavailable: action.secondOpinionUnavailable === true,
              }
            : item,
        ),
      };
    case "decisionEscalated":
      // `startedAt` is reset: the action never started automatically (Spec 0092, A3.2).
      return {
        ...state,
        items: mapAction(state.items, action.actionId, (item) => ({
          ...item,
          decision: { Confirm: { reason: action.reason, code: action.code } },
          startedAt: null,
        })),
      };
    case "actionOutput":
      return {
        ...state,
        liveOutputs: {
          ...state.liveOutputs,
          [action.actionId]: appendLiveOutput(state.liveOutputs[action.actionId], action.event),
        },
      };
    case "actionWaitingInput":
      if (action.waiting) {
        return { ...state, waitingActions: { ...state.waitingActions, [action.actionId]: true } };
      }
      return { ...state, waitingActions: withoutKey(state.waitingActions, action.actionId) };
    case "confirmationSettled":
      return {
        ...state,
        items: mapAction(state.items, action.actionId, (item) => ({
          ...item,
          responded: true,
          settled: action.reason,
        })),
      };
    case "actionResult":
      return {
        ...state,
        waitingActions: withoutKey(state.waitingActions, action.actionId),
        // Issue #325: the final result replaces the live view.
        liveOutputs: withoutKey(state.liveOutputs, action.actionId),
        items: mapAction(state.items, action.actionId, (item) => ({
          ...item,
          result: action.result,
          responded: true,
        })),
      };
    case "error": {
      // Issue #600: "Try again" only when nothing but the user's own message
      // came since the last user turn.
      let retryable = action.isRetryableCode;
      let sawUser = false;
      for (let i = state.items.length - 1; retryable && i >= 0; i--) {
        const it = state.items[i];
        if (it.type === "user") {
          sawUser = true;
          break;
        }
        if (it.type === "assistant" || it.type === "action" || it.type === "error") retryable = false;
      }
      if (!sawUser) retryable = false;
      return {
        ...append(state, {
          type: "error",
          id: action.id,
          message: action.message,
          code: action.code ?? null,
          details: action.details,
          retryable,
        }),
        retrying: null,
        aiWait: null,
      };
    }
    case "aiRetrying":
      return {
        ...state,
        retryNow: action.now,
        retrying: { attempt: action.attempt, max: action.max, deadline: action.deadline },
      };
    case "responseTruncated": {
      const last = state.items[state.items.length - 1];
      if (last?.type === "assistant") {
        return { ...state, items: [...state.items.slice(0, -1), { ...last, truncated: true }] };
      }
      // Spec 0080, A3: an empty assistant item carries the notice + "Continue".
      return append(state, { type: "assistant", id: action.newId, text: "", truncated: true });
    }
    case "injectionCheck":
      return { ...state, injectionChecks: { ...state.injectionChecks, [action.itemId]: action.state } };
    case "webActivity":
      return append(state, {
        type: "webActivity",
        id: action.id,
        activity: action.activity,
        activityId: action.activityId,
      });
    case "responseEmpty":
      return append(state, { type: "emptyResponse", id: action.id });
    case "responseRefused":
      return append(state, { type: "refusedResponse", id: action.id });
    case "webResearchUnavailable":
      return append(state, { type: "webResearchUnavailable", id: action.id });
    case "queuedMessagesSent":
      return { ...state, items: clearQueued(state.items) };
    case "contextUsage":
      return { ...state, contextUsage: { inputTokens: action.inputTokens, windowTokens: action.windowTokens } };
    case "contextCompacted":
      return append(state, { type: "contextCompacted", id: action.id });
    case "responseCancelled":
      return { ...append(state, { type: "responseCancelled", id: action.id }), retrying: null };
    case "autoContinuationLimitReached":
      return append(state, { type: "autoContinuationLimitReached", id: action.id, limit: action.limit });
    case "autoContinuationStarted":
      return { ...state, autoContinuing: true };
    case "aiBudgetWaiting":
      return append(state, { type: "aiBudgetWaiting", id: action.id, waitSeconds: action.waitSeconds });
    case "questionAsked":
      return append(state, {
        type: "question",
        id: action.id,
        questionId: action.questionId,
        question: action.question,
        options: action.options,
        multiSelect: action.multiSelect,
      });
    case "documentGenerated":
      return append(state, {
        type: "document",
        id: action.id,
        title: action.title,
        contentMarkdown: action.contentMarkdown,
      });
    case "waitTick":
      return { ...state, waitNow: action.now };
    case "retryTick":
      return { ...state, retryNow: action.now };
    case "historyLoaded":
      if (action.entries.length === 0) return state;
      return { ...state, items: [...action.entries, ...state.items] };
    case "resynced":
      return { ...state, items: mergeResyncedItems(state.items, action.rebuilt, action.pendingActionId) };
    case "itemAppended":
      return append(state, action.item);
    case "itemRemoved":
      return { ...state, items: state.items.filter((it) => it.id !== action.id) };
    case "truncatedCleared":
      return {
        ...state,
        items: state.items.map((it) =>
          it.id === action.id && it.type === "assistant" ? { ...it, truncated: false } : it,
        ),
      };
    case "actionResponded":
      return {
        ...state,
        items: mapAction(state.items, action.actionId, (item) => ({
          ...item,
          responded: true,
          userDecision: action.decision,
          startedAt: action.startedAt ?? item.startedAt,
        })),
      };
    case "actionRespondFailed":
      // Issue #1019: roll back the optimistic state.
      return {
        ...state,
        items: [
          ...mapAction(state.items, action.actionId, (item) =>
            action.unknown
              ? { ...item, responded: true, userDecision: undefined, startedAt: null, settled: "unknown" as const }
              : { ...item, responded: false, userDecision: undefined, startedAt: null },
          ),
          action.errorItem,
        ],
      };
    case "ruleAccepting":
      return {
        ...state,
        items: mapAction(state.items, action.actionId, (item) => ({
          ...item,
          responded: true,
          startedAt: action.startedAt,
        })),
      };
    case "autoContinuationStopped":
      return { ...state, autoContinuing: false, aiWait: null };
    case "turnStarted":
      // Issue #694: drop a stale wait line from the previous turn.
      return { ...state, aiWait: null };
    case "turnSettled":
      return {
        ...state,
        autoContinuing: false,
        retrying: null,
        aiWait: null,
        items: clearQueued(state.items),
      };
    case "chatCleared":
      // Issue #648: the new chat has no usage yet; keep the window size.
      return {
        ...state,
        items: [],
        contextUsage: state.contextUsage ? { ...state.contextUsage, inputTokens: 0 } : state.contextUsage,
      };
  }
}
