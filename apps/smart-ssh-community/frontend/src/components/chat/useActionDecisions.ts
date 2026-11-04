import { type Dispatch, useRef } from "react";
import { acceptAndCreateRule, commandErrorCode, commandErrorMessage, respondToAction } from "../../api/api";
import { freshId } from "../../lib/chatResync";
import type { ActionUserDecision, Scope, SuggestionPatternDto, SuggestionVerdictDto } from "../../api/types";
import type { ChatTranscriptAction } from "./chatTranscriptReducer";

/**
 * Answers to a proposed action: approve, reject, edit-then-approve
 * (`respond`) and accept-with-quick-rule (`acceptWithRule`). The calls and
 * their arguments are the only way a decision reaches the backend; nothing
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

  /** Spec 0011, Abschnitt 3: `accept_and_create_rule` legt die Regel an
   * **und** löst die Confirm-Entscheidung selbst auf (Backend) — anders als
   * `respond()` oben ruft dies also `respondToAction` nicht zusätzlich auf.
   *
   * `editedCommand`: `null`, falls der Text im Bearbeiten-Feld unverändert
   * dem KI-Vorschlag entspricht; sonst der bearbeitete Text, damit das
   * Backend über `EditThenApprove` tatsächlich DIESEN Text erneut durch die
   * Filter-Engine schickt und ausführt.
   * `startedAt`: wie bei `respond()` — dieser Pfad führt immer aus (nie
   * "deny"), sonst erschiene weder Lauf-Indikator noch Abbrechen-Button
   * (Spec 0027). */
  const acceptWithRule = (
    actionId: string,
    patterns: SuggestionPatternDto[],
    scope: Scope,
    editedCommand: string | null,
    verdict?: SuggestionVerdictDto,
  ) => {
    dispatch({ type: "ruleAccepting", actionId, startedAt: Date.now() });
    onActionSettled(sessionId);
    acceptAndCreateRule(sessionId, actionId, patterns, scope, undefined, editedCommand)
      .then(() => {
        // Issue #596: no silent failure — repeat why the command still asks.
        if (verdict && !verdict.autoExec) {
          dispatch({
            type: "itemAppended",
            item: { type: "ruleNotice", id: freshId(), reasons: verdict.reasons.map((r) => r.code) },
          });
        }
      })
      .catch((err) =>
        dispatch({
          type: "itemAppended",
          // Spec 0077, 3.1.4: Code weiterreichen statt `null`, damit ein
          // ungültiges Muster auch hier übersetzt erscheint.
          item: { type: "error", id: freshId(), message: commandErrorMessage(err), code: commandErrorCode(err) },
        }),
      );
  };

  return { respond, acceptWithRule };
}
