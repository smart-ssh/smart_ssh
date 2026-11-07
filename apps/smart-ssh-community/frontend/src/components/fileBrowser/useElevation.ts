import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, sftpElevationDisable, sftpElevationEnable } from "../../api/api";
import { onElevationAutoDisabled, onElevationRevokedByProfile } from "../../api/events";
import { showToast } from "../../buses/toastBus";
import type { ElevationResultDto, RunningOperationDto } from "../../api/types";

/** Issue #622: asked before ending elevated rights (or changing the target
 * user) while elevated operations still run. */
export interface StopPrompt {
  operations: RunningOperationDto[];
  /** What to do after the user confirmed. */
  action: { kind: "disable" } | { kind: "switch"; targetUser: string };
}

interface UseElevationOptions {
  /** Spec 0067, A5: reports the target user (or `null`) to the parent view. */
  onElevationChange?: (targetUser: string | null) => void;
  /** Reloads the current directory over the given channel user. Called with
   * the new user because the new mode has not reached the state yet. */
  reload: (targetUser: string | null) => void;
}

/**
 * Elevated (sudo) browsing mode of one session's file browser (Spec 0067):
 * the mode itself, the enable/disable flow, failure and warning state, the
 * target-user menu state and the stop prompt for running elevated operations.
 * The backend enforces the channel; this hook only requests it.
 */
export function useElevation(sessionId: string, { onElevationChange, reload }: UseElevationOptions) {
  const { t } = useTranslation();
  // Spec 0067, A5: per browser view — never the default, not persisted.
  // `null` = normal mode.
  const [elevation, setElevation] = useState<{ targetUser: string } | null>(null);
  const [enablingElevation, setEnablingElevation] = useState(false);
  // Spec 0067, A4: target user of the elevation, default root.
  const [elevationTargetUser, setElevationTargetUser] = useState("root");
  const [elevationMenuOpen, setElevationMenuOpen] = useState(false);
  const [otherUserOpen, setOtherUserOpen] = useState(false);
  const [otherUserDraft, setOtherUserDraft] = useState("root");
  const [elevationFailure, setElevationFailure] = useState<ElevationResultDto | null>(null);
  const [elevationWarning, setElevationWarning] = useState<string | null>(null);
  // Issue #1192: the elevated channel logs on the server.
  const [serverLogging, setServerLogging] = useState(false);
  // Issue #1195: sudo needed no password for the active channel.
  const [passwordless, setPasswordless] = useState(false);
  const [stopPrompt, setStopPrompt] = useState<StopPrompt | null>(null);

  const elevated = elevation !== null;
  const elevatedUser = elevation?.targetUser ?? "";
  // Spec 0067, A5: passed to every call — `null` = normal channel, otherwise
  // the expected target user (the backend checks it against the channel).
  const channelUser = elevation?.targetUser ?? null;

  // Every mode change bumps the generation — an enable that only returns
  // AFTER a disable / connection loss is discarded.
  const elevationGenerationRef = useRef(0);
  const onElevationChangeRef = useRef(onElevationChange);
  onElevationChangeRef.current = onElevationChange;
  const reloadRef = useRef(reload);
  reloadRef.current = reload;

  /** The only place that changes the mode: keeps the parent view and (on
   * ending) the backend in sync — "backend active <=> UI shows elevated". */
  const updateElevation = (next: { targetUser: string } | null, backendDone = false) => {
    elevationGenerationRef.current += 1;
    setElevation(next);
    if (next === null) {
      setElevationWarning(null);
      setServerLogging(false);
      setPasswordless(false);
    }
    onElevationChangeRef.current?.(next?.targetUser ?? null);
    if (next === null && !backendDone) {
      sftpElevationDisable(sessionId).catch((err) =>
        console.warn("Konnte erhöhten Modus nicht beenden:", err),
      );
    }
  };

  const updateElevationRef = useRef(updateElevation);
  updateElevationRef.current = updateElevation;

  // A still-open elevated channel (e.g. from an earlier view of this session)
  // is closed on open — the browser always starts normal.
  useEffect(() => {
    sftpElevationDisable(sessionId).catch((err) =>
      console.warn("Konnte erhöhten Modus nicht zurücksetzen:", err),
    );
  }, [sessionId]);

  // Issue #1193: the backend switched the channel off after inactivity. The
  // channel is already gone, so the mode ends like after a manual switch-off
  // (no disable call), and a persistent toast says why.
  const elevatedRef = useRef(false);
  elevatedRef.current = elevated;
  useEffect(() => {
    const unlisten = onElevationAutoDisabled((event) => {
      if (event.sessionId !== sessionId || !elevatedRef.current) return;
      updateElevationRef.current(null, true);
      reloadRef.current(null);
      showToast({
        kind: "info",
        message: t("fileElevation.idleSwitchedOff", { minutes: event.minutes }),
      });
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [sessionId, t]);

  // Issue #1194: saving the server profile with elevated rights turned off
  // revoked the channel in the backend; leave elevated mode with a toast.
  useEffect(() => {
    const unlisten = onElevationRevokedByProfile((event) => {
      if (event.sessionId !== sessionId || !elevatedRef.current) return;
      updateElevationRef.current(null, true);
      reloadRef.current(null);
      showToast({ kind: "info", message: t("fileElevation.revokedByProfile") });
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [sessionId, t]);

  const enableElevation = async (targetUser: string, confirmStop = false) => {
    setEnablingElevation(true);
    const generation = elevationGenerationRef.current;
    try {
      const result = await sftpElevationEnable(sessionId, targetUser.trim() || null, confirmStop);
      if (result.runningOperations && result.runningOperations.length > 0) {
        setStopPrompt({
          operations: result.runningOperations,
          action: { kind: "switch", targetUser },
        });
        return;
      }
      if (generation !== elevationGenerationRef.current) {
        // Switched off / connection lost meanwhile — do not show elevated
        // again, close the backend channel.
        if (result.active) updateElevation(null);
        return;
      }
      if (result.active) {
        updateElevation({ targetUser: result.targetUser });
        setElevationWarning(result.warning?.path ?? null);
        setServerLogging(result.serverLogging === true);
        setPasswordless(result.sudoAuth === "nopasswd");
        reloadRef.current(result.targetUser);
      } else {
        setElevationFailure(result);
      }
    } catch (err) {
      setElevationFailure({
        active: false,
        targetUser: "root",
        sftpServerPath: null,
        failure: { kind: "checkFailed", sudoersLine: null, detail: commandErrorMessage(err) },
        warning: null,
        runningOperations: [],
        serverLogging: false,
        sudoAuth: null,
      });
    } finally {
      setEnablingElevation(false);
    }
  };

  const handleToggleElevation = async (targetUser: string = "root") => {
    setElevationMenuOpen(false);
    setOtherUserOpen(false);
    if (elevated) {
      // Issue #622: with running elevated operations the backend changes
      // nothing and lists them; the user confirms (or cancels) first.
      try {
        const result = await sftpElevationDisable(sessionId, false);
        if (!result.disabled) {
          setStopPrompt({ operations: result.runningOperations, action: { kind: "disable" } });
          return;
        }
      } catch (err) {
        console.warn("Konnte erhöhten Modus nicht beenden:", err);
      }
      updateElevation(null, true);
      reloadRef.current(null);
      return;
    }
    await enableElevation(targetUser);
  };

  /** Issue #622: "End elevated rights" in the dialog — stops the listed
   * operations, then closes the channel. */
  const confirmStopPrompt = async () => {
    const prompt = stopPrompt;
    setStopPrompt(null);
    if (!prompt) return;
    if (prompt.action.kind === "disable") {
      try {
        await sftpElevationDisable(sessionId, true);
      } catch (err) {
        console.warn("Konnte erhöhten Modus nicht beenden:", err);
      }
      updateElevation(null, true);
      reloadRef.current(null);
    } else {
      await enableElevation(prompt.action.targetUser, true);
    }
  };

  return {
    elevation,
    elevated,
    elevatedUser,
    channelUser,
    enablingElevation,
    elevationTargetUser,
    setElevationTargetUser,
    elevationMenuOpen,
    setElevationMenuOpen,
    otherUserOpen,
    setOtherUserOpen,
    otherUserDraft,
    setOtherUserDraft,
    elevationFailure,
    setElevationFailure,
    elevationWarning,
    serverLogging,
    passwordless,
    stopPrompt,
    setStopPrompt,
    updateElevation,
    handleToggleElevation,
    confirmStopPrompt,
  };
}
