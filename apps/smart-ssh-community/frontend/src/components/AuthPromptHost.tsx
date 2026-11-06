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
 * whichever tab is active. A second round of the same login arrives as a
 * new event and replaces the dialog. `auth-prompt-ended` closes the dialog
 * for the matching prompt id only (timeout, or the connect was aborted).
 *
 * The answers are passed straight to `answer_auth_prompt`; they are never
 * stored here, and the dialog is closed (state cleared) before the call.
 */
export function AuthPromptHost() {
  const { t } = useTranslation();
  const [pending, setPendingState] = useState<AuthPromptNeededEvent | null>(null);
  const pendingRef = useRef<AuthPromptNeededEvent | null>(null);
  const setPending = useCallback((event: AuthPromptNeededEvent | null) => {
    pendingRef.current = event;
    setPendingState(event);
  }, []);

  useEffect(() => {
    const unlisten = onAuthPromptNeeded((event) => setPending(event));
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [setPending]);

  useEffect(() => {
    const unlisten = onAuthPromptEnded((event) => {
      const current = pendingRef.current;
      if (!current || current.promptId !== event.promptId) return;
      setPending(null);
      if (event.reason === "timed_out") {
        showToast({ kind: "info", message: t("authPrompt.expired") });
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [setPending, t]);

  if (!pending) return null;
  const { promptId } = pending;

  const report = (err: unknown) =>
    showToast({ kind: "error", message: commandErrorText(t, err) });

  return (
    <AuthPromptDialog
      key={promptId}
      event={pending}
      onSubmit={(answers) => {
        setPending(null);
        answerAuthPrompt(promptId, answers).catch(report);
      }}
      onCancel={() => {
        setPending(null);
        cancelAuthPrompt(promptId).catch(report);
      }}
    />
  );
}
