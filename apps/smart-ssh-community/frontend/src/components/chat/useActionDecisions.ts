import { type Dispatch, useRef } from "react";
import { commandErrorCode, commandErrorMessage, respondToAction } from "../../api/api";
import { freshId } from "../../lib/chatResync";
import type { ActionUserDecision } from "../../api/types";
import type { ChatTranscriptAction } from "./chatTranscriptReducer";

/**
 * Answers to a proposed action: approve, reject and edit-then-approve
 * (`respond`). The call and
 * its arguments are the only way a decision reaches the backend; nothing
 * here approves on its own.
 */
export function useActionDecisions(
  sessionId: string,
  dispatch: Dispatch<ChatTranscriptAction>,
  onActionSettled: (sessionId: string) => void,
) {
  // Actions whose decision is on its way: a second click sends nothing.
  const pendingRef = useRef(new Set<string>());

  const respond = (actionId: string, decision: ActionUserDecision) => {
    if (pendingRef.current.has(actionId)) return;
    pendingRef.current.add(actionId);
    // Spec 0027: die tatsächliche Ausführung beginnt jetzt (nicht bei
    // "deny", das führt nie etwas aus) — Startzeitpunkt für den
    // Lauf-Indikator.
    const startedAt =
      decision.decision === "approve" || decision.decision === "editThenApprove"
        ? Date.now()
        : null;
    dispatch({ type: "actionResponded", actionId, decision: decision.decision, startedAt });
    onActionSettled(sessionId);
    respondToAction(sessionId, actionId, decision).catch((err) => {
      // The request stays open (or expired), so a new decision is possible.
      pendingRef.current.delete(actionId);
      // Issue #1019: the optimistic state is rolled back. An unknown
      // confirmation was settled by the backend: the card shows "expired",
      // never "approved". Any other error leaves the request open.
      const unknown = commandErrorCode(err) === "ACTION_CONFIRMATION_UNKNOWN";
      dispatch({
        type: "actionRespondFailed",
        actionId,
        unknown,
        errorItem: { type: "error", id: freshId(), message: commandErrorMessage(err), code: null },
      });
    });
  };

  return { respond };
}
