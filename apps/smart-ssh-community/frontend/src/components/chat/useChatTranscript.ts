import { useEffect, useReducer, useRef, type Dispatch } from "react";
import { getChatHistory, getChatResync } from "../../api/api";
import { isRetryableAiError } from "../../api/errorCodes";
import { actionItemFromProposed, freshId, historyEntryToItem } from "../../lib/chatResync";
import {
  onActionConfirmationSettled,
  onActionDecisionEscalated,
  onAiBudgetWaiting,
  onChatActionOutput,
  onChatActionProposed,
  onChatActionResult,
  onChatActionWaitingInput,
  onChatAiRequestStarted,
  onChatAiRetrying,
  onChatAutoContinuationLimitReached,
  onChatAutoContinuationStarted,
  onChatContextCompacted,
  onChatContextUsage,
  onChatDocumentGenerated,
  onChatError,
  onChatInjectionCheck,
  onChatQuestionAsked,
  onChatQueuedMessagesSent,
  onChatResponseCancelled,
  onChatResponseEmpty,
  onChatResponseRefused,
  onChatResponseTruncated,
  onChatTextDelta,
  onChatWebActivity,
  onChatWebResearchUnavailable,
  onRiskAssessmentUpdated,
} from "../../api/events";
import type { InjectionCheckState } from "../../api/types";
import {
  chatTranscriptReducer,
  initialChatTranscriptState,
  type ChatTranscriptAction,
  type ChatTranscriptState,
} from "./chatTranscriptReducer";

export interface UseChatTranscriptOptions {
  /** Read through a ref: changing it never resubscribes the chat events. */
  riskSecondOpinionEnabled: boolean;
  /** Called when the backend settles a confirmation (Spec 0017, Abschnitt 5). */
  onActionSettled: (sessionId: string) => void;
}

/**
 * Subscribes to the chat events of one session and reduces them into the
 * visible transcript (see `chatTranscriptReducer`). The subscription effect
 * depends only on `sessionId`.
 */
export function useChatTranscript(
  sessionId: string,
  options: UseChatTranscriptOptions,
): { state: ChatTranscriptState; dispatch: Dispatch<ChatTranscriptAction> } {
  const [state, dispatch] = useReducer(chatTranscriptReducer, undefined, () =>
    initialChatTranscriptState(),
  );
  const optionsRef = useRef(options);
  useEffect(() => {
    optionsRef.current = options;
  });

  // Tick the "waiting" / "retry" lines while they are shown.
  const waiting = state.aiWait !== null;
  const retrying = state.retrying !== null;
  useEffect(() => {
    if (!waiting) return;
    const id = window.setInterval(() => dispatch({ type: "waitTick", now: Date.now() }), 1000);
    return () => window.clearInterval(id);
  }, [waiting]);
  useEffect(() => {
    if (!retrying) return;
    const id = window.setInterval(() => dispatch({ type: "retryTick", now: Date.now() }), 250);
    return () => window.clearInterval(id);
  }, [retrying]);

  // Spec 0034, Abschnitt 6/8: seeds the history of a resumed session.
  useEffect(() => {
    getChatHistory(sessionId)
      .then((entries) => {
        if (entries.length === 0) return;
        dispatch({ type: "historyLoaded", entries: entries.map(historyEntryToItem) });
      })
      .catch(() => {
        // Best-effort: start without the old history rather than unusable.
      });
  }, [sessionId]);

  // Issue #1020, Spec 0114 §4: rebuild the chat after the app returns from
  // the background (events may have been missed).
  useEffect(() => {
    let cancelled = false;
    const onVisible = () => {
      if (document.visibilityState !== "visible") return;
      getChatResync(sessionId)
        .then((resync) => {
          if (cancelled) return;
          const rebuilt = resync.history.map(historyEntryToItem);
          const pending = resync.pendingProposal;
          if (pending) {
            rebuilt.push(actionItemFromProposed(pending, optionsRef.current.riskSecondOpinionEnabled));
          }
          dispatch({ type: "resynced", rebuilt, pendingActionId: pending?.actionId ?? null });
        })
        .catch(() => {
          // Best-effort like the initial history load: keep what is shown.
        });
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      cancelled = true;
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [sessionId]);

  useEffect(() => {
    // Spec 0061 Follow-up: timers for removing the `aiBudgetWaiting` card must
    // not outlive the session / unmount.
    const pendingBudgetTimers: number[] = [];
    const unlisten = [
      onChatAiRequestStarted((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "aiRequestStarted", provider: e.providerName, now: Date.now() });
      }),
      onChatTextDelta((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "textDelta", delta: e.delta, now: Date.now(), newId: freshId() });
      }),
      onChatActionProposed((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({
          type: "actionProposed",
          item: actionItemFromProposed(e, optionsRef.current.riskSecondOpinionEnabled),
        });
      }),
      onRiskAssessmentUpdated((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({
          type: "riskAssessmentUpdated",
          actionId: e.actionId,
          dataRisk: e.dataRisk,
          reason: e.reason,
          secondOpinionUnavailable: e.secondOpinionUnavailable,
        });
      }),
      onActionDecisionEscalated((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "decisionEscalated", actionId: e.actionId, reason: e.reason, code: e.code });
      }),
      onChatActionOutput((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "actionOutput", actionId: e.actionId, event: e });
      }),
      onChatActionWaitingInput((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "actionWaitingInput", actionId: e.actionId, waiting: e.waiting });
      }),
      onActionConfirmationSettled((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "confirmationSettled", actionId: e.actionId, reason: e.reason });
        optionsRef.current.onActionSettled(sessionId);
      }),
      onChatActionResult((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "actionResult", actionId: e.actionId, result: e.result });
      }),
      onChatError((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({
          type: "error",
          id: freshId(),
          message: e.message,
          code: e.code ?? null,
          details: {
            providerErrorType: e.providerErrorType,
            httpStatus: e.httpStatus,
            providerMessage: e.providerMessage,
            providerName: e.providerName,
            timeoutSecs: e.timeoutSecs,
            accessReturns: e.accessReturns,
          },
          isRetryableCode: isRetryableAiError(e.code),
        });
      }),
      onChatAiRetrying((e) => {
        if (e.sessionId !== sessionId) return;
        const now = Date.now();
        dispatch({
          type: "aiRetrying",
          attempt: e.attempt,
          max: e.maxAttempts,
          deadline: now + e.delaySecs * 1000,
          now,
        });
      }),
      onChatResponseTruncated((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "responseTruncated", newId: freshId() });
      }),
      onChatInjectionCheck((e) => {
        if (e.sessionId !== sessionId) return;
        const { sessionId: _s, itemId, ...rest } = e;
        dispatch({ type: "injectionCheck", itemId, state: rest as InjectionCheckState });
      }),
      onChatWebActivity((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "webActivity", id: freshId(), activity: e.activity, activityId: e.activityId });
      }),
      onChatResponseEmpty((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "responseEmpty", id: freshId() });
      }),
      onChatResponseRefused((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "responseRefused", id: freshId() });
      }),
      onChatWebResearchUnavailable((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "webResearchUnavailable", id: freshId() });
      }),
      onChatQueuedMessagesSent((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "queuedMessagesSent" });
      }),
      onChatContextUsage((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "contextUsage", inputTokens: e.inputTokens, windowTokens: e.windowTokens });
      }),
      onChatContextCompacted((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "contextCompacted", id: freshId() });
      }),
      onChatResponseCancelled((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "responseCancelled", id: freshId() });
      }),
      onChatAutoContinuationLimitReached((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "autoContinuationLimitReached", id: freshId(), limit: e.limit });
      }),
      onChatAutoContinuationStarted((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({ type: "autoContinuationStarted" });
      }),
      onAiBudgetWaiting((e) => {
        if (e.sessionId !== sessionId) return;
        const id = freshId();
        dispatch({ type: "aiBudgetWaiting", id, waitSeconds: e.waitSeconds });
        // Spec 0061 §4: the card vanishes once the request goes out. There is
        // no backend "wait over" event (ADR 0054), so a client-side timer
        // approximates it from the known wait time.
        pendingBudgetTimers.push(
          window.setTimeout(() => dispatch({ type: "itemRemoved", id }), Math.max(e.waitSeconds, 1) * 1000),
        );
      }),
      onChatQuestionAsked((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({
          type: "questionAsked",
          id: freshId(),
          questionId: e.questionId,
          question: e.question,
          options: e.options,
          multiSelect: e.multiSelect,
        });
      }),
      onChatDocumentGenerated((e) => {
        if (e.sessionId !== sessionId) return;
        dispatch({
          type: "documentGenerated",
          id: freshId(),
          title: e.title,
          contentMarkdown: e.contentMarkdown,
        });
      }),
    ];

    return () => {
      unlisten.forEach((p) => p.then((unlistenFn) => unlistenFn()));
      pendingBudgetTimers.forEach((id) => window.clearTimeout(id));
    };
  }, [sessionId]);

  return { state, dispatch };
}
