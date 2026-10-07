import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, confirmHostKey } from "../api";
import { translateErrorCode } from "../errorCodes";
import { onHostKeyVerificationEnded, onHostKeyVerificationNeeded } from "../events";
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
 * (`HostKeyWait::TimedOut`). Schlägt `confirmHostKey` fehl, erscheint ein
 * Fehler-Toast.
 *
 * Issue #37 / ADR 0107: `host-key-verification-ended` schließt die Abfrage,
 * sobald das Backend nicht mehr auf sie wartet — aber nur, wenn
 * `sessionId` UND `promptId` zur angezeigten Abfrage passen. Ein älteres
 * Ende-Ereignis (andere Session oder ältere Abfrage derselben Session)
 * schließt nie eine neuere Abfrage. Bei Timeout erscheint ein Hinweis
 * (kein Fehler); `confirmHostKey` wird dabei nicht aufgerufen.
 */
export function HostKeyPromptHost() {
  const { t } = useTranslation();
  const [pending, setPendingState] = useState<HostKeyVerificationNeededEvent | null>(null);
  // Spiegel des angezeigten Zustands für den Ende-Listener: der vergleicht
  // synchron gegen die aktuell angezeigte Abfrage, ohne Seiteneffekte (Toast)
  // in einen `setState`-Updater zu legen.
  const pendingRef = useRef<HostKeyVerificationNeededEvent | null>(null);
  const setPending = useCallback((event: HostKeyVerificationNeededEvent | null) => {
    pendingRef.current = event;
    setPendingState(event);
  }, []);

  useEffect(() => {
    const unlisten = onHostKeyVerificationNeeded((event) => setPending(event));
    return () => {
      unlisten.then((unlistenFn) => unlistenFn());
    };
  }, [setPending]);

  useEffect(() => {
    const unlisten = onHostKeyVerificationEnded((event) => {
      const current = pendingRef.current;
      if (
        !current ||
        current.sessionId !== event.sessionId ||
        current.promptId !== event.promptId
      ) {
        return;
      }
      setPending(null);
      if (event.reason === "timed_out") {
        showToast({
          kind: "info",
          message: t("hostKeyDialog.expired", { host: current.host, port: current.port }),
        });
      }
    });
    return () => {
      unlisten.then((unlistenFn) => unlistenFn());
    };
  }, [setPending, t]);

  useEffect(() => subscribeHostKeyPromptClear(() => setPending(null)), [setPending]);

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
