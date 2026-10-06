import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, confirmHostKey } from "../api";
import { translateErrorCode } from "../errorCodes";
import { onHostKeyVerificationNeeded } from "../events";
import { subscribeHostKeyPromptClear } from "../hostKeyPromptBus";
import { showToast } from "../toastBus";
import type { HostKeyVerificationNeededEvent } from "../types";
import { HostKeyDialog } from "./HostKeyDialog";

/**
 * Issue #12 / ADR 0104: einziger Empfänger von
 * `host-key-verification-needed` (Spec 0007, Spec 0100, ADR 0091). Wird von
 * `App` neben `ToastHost` außerhalb aller Tab-/Session-Zweige gerendert,
 * damit eine Host-Key-Abfrage — auch für einen vom Backend ausgelösten
 * Verbindungsaufbau, z. B. über MCP — unabhängig davon erscheint, welcher
 * Tab gerade aktiv ist. Vorher lebte der Listener in `ServerList`, das bei
 * aktivem "Verwalten"-/"Filter-Regeln"-Tab nicht gemountet ist.
 *
 * Eine Abfrage zur Zeit: ein neueres Event ersetzt das angezeigte; das
 * ersetzte läuft im Backend in den Timeout und gilt dort als Ablehnung
 * (`HostKeyWait::TimedOut`). Schlägt `confirmHostKey` fehl (etwa weil das
 * Backend bereits per Timeout abgelehnt hat), erscheint ein Fehler-Toast.
 */
export function HostKeyPromptHost() {
  const { t } = useTranslation();
  const [pending, setPending] = useState<HostKeyVerificationNeededEvent | null>(null);

  useEffect(() => {
    const unlisten = onHostKeyVerificationNeeded((event) => setPending(event));
    return () => {
      unlisten.then((unlistenFn) => unlistenFn());
    };
  }, []);

  useEffect(() => subscribeHostKeyPromptClear(() => setPending(null)), []);

  const handleDecision = async (decision: Parameters<typeof confirmHostKey>[1]) => {
    if (!pending) return;
    const sessionId = pending.sessionId;
    setPending(null);
    try {
      await confirmHostKey(sessionId, decision);
    } catch (err) {
      showToast({
        kind: "error",
        message: translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)),
      });
    }
  };

  if (!pending) return null;
  return <HostKeyDialog event={pending} onDecision={handleDecision} />;
}
