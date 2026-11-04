import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { cancelConnect, commandErrorCode, testConnection, trustHostKey } from "../../api/api";
import { commandErrorText } from "../../api/errorCodes";
import { onConnectProgress } from "../../api/events";
import type {
  ConnectStepSnapshot,
  HostKeyInfo,
  ServerInput,
  TestConnectionReport,
} from "../../api/types";

interface Options {
  /** `null` = the server is not saved yet. */
  serverId: string | null;
  /** Reads the current form input at call time. */
  buildInput: () => ServerInput;
  setError: (message: string | null) => void;
}

/** The "test connection" flow of the server form, including the host-key
 * prompt: the key is only trusted after the user's explicit decision
 * (`handleTrustAndRetest`). */
export function useConnectionTest({ serverId, buildInput, setError }: Options) {
  const { t } = useTranslation();
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestConnectionReport | null>(null);
  // Issue #633: the dialog with the live step list; `null` = closed.
  const [testDialog, setTestDialog] = useState<{ steps: ConnectStepSnapshot[] } | null>(null);
  // Attempt id of the test the dialog currently shows; a result or error of
  // any other (cancelled) attempt is ignored.
  const testAttemptRef = useRef<string | null>(null);
  const [pendingHostKey, setPendingHostKey] = useState<HostKeyInfo | null>(null);

  /** Forgets the last result and a pending host-key prompt (server reload). */
  const resetResult = () => {
    setTestResult(null);
    setPendingHostKey(null);
  };

  const runTest = async () => {
    const attemptId = crypto.randomUUID();
    testAttemptRef.current = attemptId;
    setTesting(true);
    setError(null);
    setTestResult(null);
    setTestDialog({ steps: [] });
    let unlisten: (() => void) | undefined;
    try {
      // Subscribe before the command starts so no early step is missed.
      unlisten = await onConnectProgress((event) => {
        if (event.attemptId === attemptId) setTestDialog({ steps: event.steps });
      });
      const result = await testConnection(buildInput(), attemptId, serverId ?? undefined);
      if (testAttemptRef.current !== attemptId) return;
      setTestResult(result);
      if (result.kind === "hostKeyUnknown" || result.kind === "hostKeyMismatch") {
        // The host key prompt takes over from the progress dialog; the badge
        // in the form keeps the result.
        testAttemptRef.current = null;
        setTestDialog(null);
        setPendingHostKey({
          host: result.host,
          port: result.port,
          kind: result.kind === "hostKeyUnknown" ? "unknown" : "mismatch",
          fingerprint: result.kind === "hostKeyUnknown" ? result.fingerprint : result.actualFingerprint,
          expectedFingerprint: result.kind === "hostKeyMismatch" ? result.expectedFingerprint : null,
          keyType: result.keyType,
        });
      }
    } catch (err) {
      if (testAttemptRef.current !== attemptId) return;
      testAttemptRef.current = null;
      setTestDialog(null);
      if (commandErrorCode(err) !== "SSH_CONNECT_CANCELLED") {
        setError(commandErrorText(t, err));
      }
    } finally {
      unlisten?.();
      if (testAttemptRef.current === attemptId || testAttemptRef.current === null) setTesting(false);
    }
  };

  // Cancel (button or Escape) while the test runs: end it without an error.
  const cancelTest = () => {
    const attemptId = testAttemptRef.current;
    testAttemptRef.current = null;
    setTestDialog(null);
    setTesting(false);
    if (attemptId) void cancelConnect(attemptId).catch(() => undefined);
  };

  const closeTestDialog = () => {
    testAttemptRef.current = null;
    setTestDialog(null);
  };

  const handleTrustAndRetest = async () => {
    if (testResult?.kind !== "hostKeyUnknown" && testResult?.kind !== "hostKeyMismatch") return;
    const { host: h, port: p, rawKey } = testResult;
    setPendingHostKey(null);
    try {
      await trustHostKey(h, p, rawKey);
      await runTest();
    } catch (err) {
      setError(commandErrorText(t, err));
    }
  };

  return {
    testing,
    testResult,
    testDialog,
    pendingHostKey,
    setPendingHostKey,
    resetResult,
    runTest,
    cancelTest,
    closeTestDialog,
    handleTrustAndRetest,
  };
}
