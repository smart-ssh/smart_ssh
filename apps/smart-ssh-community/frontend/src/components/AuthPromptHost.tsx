import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { answerAuthPrompt, cancelAuthPrompt } from "../api/api";
import { commandErrorText } from "../api/errorCodes";
import { onAuthPromptEnded, onAuthPromptNeeded } from "../api/events";
import type { AuthPromptNeededEvent } from "../api/types";
import { showToast } from "../buses/toastBus";
import { AuthPromptDialog } from "./AuthPromptDialog";

/**
 * Issue #1185 (Spec 0005): the only receiver of `auth-prompt-needed`,
 * mounted at the `App` root like `HostKeyPromptHost`, so the dialog shows
 * whichever tab is active. Issue #1190: prompts of different connects queue
 * and are shown one after another in arrival order; a later round of the
 * same login (same `connectId`, new prompt id) replaces that connect's own
 * prompt in place. `auth-prompt-ended` removes the matching prompt by id,
 * whether it is shown or waiting.
 *
 * The answers are passed straight to `answer_auth_prompt`; they are never
 * stored here, and the dialog is closed (state cleared) before the call.
 */
export function AuthPromptHost() {
  const { t } = useTranslation();
  const [queue, setQueueState] = useState<AuthPromptNeededEvent[]>([]);
  const queueRef = useRef<AuthPromptNeededEvent[]>([]);
  const setQueue = useCallback((next: AuthPromptNeededEvent[]) => {
    queueRef.current = next;
    setQueueState(next);
  }, []);

  useEffect(() => {
    const unlisten = onAuthPromptNeeded((event) => {
      const current = queueRef.current;
      const i = current.findIndex((p) => p.connectId === event.connectId);
      setQueue(i >= 0 ? current.map((p, j) => (j === i ? event : p)) : [...current, event]);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [setQueue]);

  useEffect(() => {
    const unlisten = onAuthPromptEnded((event) => {
      const current = queueRef.current;
      if (!current.some((p) => p.promptId === event.promptId)) return;
      setQueue(current.filter((p) => p.promptId !== event.promptId));
      if (event.reason === "timed_out") {
        showToast({ kind: "info", message: t("authPrompt.expired") });
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [setQueue, t]);

  const pending = queue[0];
  const dismiss = (promptId: string) =>
    setQueue(queueRef.current.filter((p) => p.promptId !== promptId));

  if (!pending) return null;
  const { promptId } = pending;

  const report = (err: unknown) =>
    showToast({ kind: "error", message: commandErrorText(t, err) });

  return (
    <AuthPromptDialog
      key={promptId}
      event={pending}
      onSubmit={(answers) => {
        dismiss(promptId);
        answerAuthPrompt(promptId, answers).catch(report);
      }}
      onCancel={() => {
        dismiss(promptId);
        cancelAuthPrompt(promptId).catch(report);
      }}
    />
  );
}
