// Issue #1020, Spec 0114 §4: rebuilding the chat from the backend after the
// app returns from the background.
import type { ChatItem } from "./components/ChatPanel";
import type { ChatActionProposedEvent, ChatHistoryEntryDto } from "./types";

/** History plus the confirmation that still waits (`chat-action-proposed`
 * payload), to rebuild a chat after missed events. */
export interface ChatResyncDto {
  history: ChatHistoryEntryDto[];
  pendingProposal: ChatActionProposedEvent | null;
}

let nextId = 0;
export const freshId = () => `item-${nextId++}`;

/** Rebuilds one chat item from a stored history entry (resumed chat and the
 * re-sync after the app returns from the background, issue #1020). */
export function historyEntryToItem(entry: ChatHistoryEntryDto): ChatItem {
  switch (entry.type) {
    case "text":
      // Issue #1020: an action result is not something the assistant said.
      if (entry.role === "action_result") {
        return { type: "historyActionResult", id: freshId(), text: entry.text };
      }
      return {
        type: entry.role === "user" ? "user" : "assistant",
        id: freshId(),
        text: entry.text,
      };
    case "commandResult":
      return {
        type: "historyCommandResult",
        id: freshId(),
        command: entry.command,
        stdout: entry.stdout,
        stderr: entry.stderr,
        exitCode: entry.exitCode,
        cancelled: entry.cancelled,
        truncated: entry.truncated,
      };
    case "actionRejected":
      return {
        type: "historyRejected",
        id: freshId(),
        command: entry.command,
        reason: entry.reason,
      };
    case "webActivity":
      return { type: "webActivity", id: freshId(), activity: entry.activity };
    case "extensionParts":
      // Issue #504: shown like attachments — label and kind only,
      // never content, without asking any extension.
      return {
        type: "historyAttachments",
        id: freshId(),
        items: entry.items.map((p) => ({ name: p.label, kind: p.media })),
      };
    case "question":
      return {
        type: "question",
        id: freshId(),
        questionId: entry.questionId,
        question: entry.question,
        options: entry.options,
        multiSelect: entry.multiSelect,
      };
  }
}

/** Card for a `chat-action-proposed` payload (live event and re-sync). */
export function actionItemFromProposed(
  event: ChatActionProposedEvent,
  riskSecondOpinionEnabled: boolean,
): ChatItem {
  // Spec 0026, Abschnitt 3/4: eine Zweitmeinung ist nur zu erwarten, wenn
  // sie aktiviert ist, die Einschätzung noch Raum nach oben hat und sie noch
  // nicht gelaufen ist.
  const riskSecondOpinionPending =
    riskSecondOpinionEnabled &&
    event.riskAssessment !== null &&
    event.riskAssessment.dataRisk !== "red" &&
    !event.riskAssessment.aiReviewed;
  return {
    type: "action",
    id: freshId(),
    actionId: event.actionId,
    action: event.action,
    decision: event.decision,
    responded: false,
    previousNoteContent: event.previousNoteContent,
    usesStoredSudoPassword: event.usesStoredSudoPassword,
    previousFileContent: event.previousFileContent,
    previousFileSize: event.previousFileSize,
    targetName: event.targetName,
    riskAssessment: event.riskAssessment,
    riskSecondOpinionPending,
    secondOpinionUnavailable: false,
    // Spec 0027: bei AutoExec beginnt die Ausführung sofort.
    startedAt: event.decision === "AutoExec" ? Date.now() : null,
    origin: event.origin,
    autoExecRules: event.autoExecRules,
  };
}

/** Live-only notices: the backend history holds no entry for them, so a
 * rebuilt list never contains them (issue #1041). `aiBudgetWaiting` is
 * transient and stays out. */
const LIVE_ONLY_TYPES: ReadonlySet<ChatItem["type"]> = new Set([
  "error",
  "ruleNotice",
  "contextCompacted",
  "autoContinuationLimitReached",
  "emptyResponse",
  "refusedResponse",
  "webResearchUnavailable",
  "responseCancelled",
]);

/** Issue #1020: the chat after a re-sync — stored history, the still-waiting
 * confirmation, and the live items the backend history does not hold yet
 * (running commands, the partial answer being streamed). */
export function mergeResyncedItems(
  current: ChatItem[],
  rebuilt: ChatItem[],
  pendingActionId: string | null,
): ChatItem[] {
  const last = current[current.length - 1];
  const rebuiltLast = rebuilt[rebuilt.length - 1];
  const keepStreamingTail =
    last?.type === "assistant" &&
    !(
      rebuiltLast?.type === "assistant" &&
      rebuiltLast.text.startsWith(last.text)
    );
  const carried = current.filter(
    (item) =>
      LIVE_ONLY_TYPES.has(item.type) ||
      (item === last
        ? keepStreamingTail
        : item.type === "action" &&
          !item.result &&
          item.actionId !== pendingActionId &&
          (item.decision === "AutoExec" ||
            (item.responded && item.userDecision !== "deny"))),
  );
  return [...rebuilt, ...carried];
}
