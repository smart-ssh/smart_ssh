import { useCallback, useEffect, useRef, useState } from "react";
import { commandErrorCode } from "./api";
import {
  cancelSpeechModelDownload,
  cancelVoiceRecording,
  getSelectedSpeechModel,
  getSpeechModelStatus,
  onSpeechModelStatus,
  onSpeechPartialText,
  onSpeechRecordingState,
  startSpeechModelDownload,
  startVoiceRecording,
  stopVoiceRecording,
  type SpeechModelId,
} from "./voiceApi";
import { voiceErrorKey } from "./voiceInputSettings";

export type VoicePhase = "idle" | "recording" | "transcribing";

export interface ConsentState {
  model: SpeechModelId;
  /** `null` = waiting for the user's decision; otherwise download progress. */
  progress: { done: number; total: number } | null;
}

/** Spec 0108 §5.2: the caller shows provisional text while recording. */
export interface VoiceHandlers {
  /** A recording started. */
  onStart: () => void;
  /** Provisional text so far (replaces the previous provisional text). */
  onPartial: (text: string) => void;
  /** The final text; empty when nothing was recognised. */
  onFinal: (text: string) => void;
  /** The final pass failed: the provisional text stays, the input unlocks. */
  onFailed: () => void;
  /** Discarded (Esc, leaving the chat): remove all provisional text. */
  onCancel: () => void;
}

/** Spec 0108 §5: records, shows live text and hands the final text to
 * `onFinal`. The microphone and the model download are only ever started by
 * `toggle` / `confirmDownload`, i.e. by a user action. */
export function useVoiceInput(handlers: VoiceHandlers) {
  const [phase, setPhase] = useState<VoicePhase>("idle");
  const [elapsed, setElapsed] = useState(0);
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const [consent, setConsent] = useState<ConsentState | null>(null);
  const phaseRef = useRef<VoicePhase>("idle");
  const consentRef = useRef<ConsentState | null>(null);
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;
  const lastRevisionRef = useRef(0);

  const updatePhase = (p: VoicePhase) => {
    phaseRef.current = p;
    setPhase(p);
  };
  const updateConsent = (c: ConsentState | null) => {
    consentRef.current = c;
    setConsent(c);
  };

  const begin = useCallback(async () => {
    setErrorKey(null);
    try {
      await startVoiceRecording();
      setElapsed(0);
      updatePhase("recording");
      handlersRef.current.onStart();
    } catch (err) {
      updatePhase("idle");
      if (commandErrorCode(err) === "speech_no_model") {
        void openConsent();
      } else {
        setErrorKey(voiceErrorKey(commandErrorCode(err)));
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const openConsent = async () => {
    try {
      const model = await getSelectedSpeechModel();
      updateConsent({ model, progress: null });
    } catch (err) {
      setErrorKey(voiceErrorKey(commandErrorCode(err)));
    }
  };

  const finish = useCallback(async () => {
    if (phaseRef.current === "transcribing") return;
    updatePhase("transcribing");
    try {
      const text = await stopVoiceRecording();
      handlersRef.current.onFinal(text);
    } catch (err) {
      setErrorKey(voiceErrorKey(commandErrorCode(err)));
      // `NotRecording` after a cancel elsewhere also lands here; the
      // handler keeps whatever provisional text is there.
      handlersRef.current.onFailed();
    } finally {
      updatePhase("idle");
    }
  }, []);

  const cancel = useCallback(() => {
    if (phaseRef.current !== "recording") return;
    updatePhase("idle");
    handlersRef.current.onCancel();
    void cancelVoiceRecording().catch(() => {});
  }, []);

  const toggle = useCallback(async () => {
    if (phaseRef.current === "transcribing" || consentRef.current) return;
    if (phaseRef.current === "recording") {
      await finish();
      return;
    }
    setErrorKey(null);
    try {
      const model = await getSelectedSpeechModel();
      const status = await getSpeechModelStatus(model);
      if (status.state !== "ready") {
        updateConsent({ model, progress: null });
        return;
      }
    } catch (err) {
      setErrorKey(voiceErrorKey(commandErrorCode(err)));
      return;
    }
    await begin();
  }, [begin, finish]);

  const confirmDownload = useCallback(async () => {
    const current = consentRef.current;
    if (!current || current.progress) return;
    updateConsent({ ...current, progress: { done: 0, total: 0 } });
    try {
      await startSpeechModelDownload(current.model);
    } catch (err) {
      updateConsent(null);
      setErrorKey(voiceErrorKey(commandErrorCode(err)));
    }
  }, []);

  const dismissConsent = useCallback(() => {
    const current = consentRef.current;
    if (!current) return;
    updateConsent(null);
    if (current.progress) void cancelSpeechModelDownload(current.model).catch(() => {});
  }, []);

  // Download progress and result.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onSpeechModelStatus((ev) => {
      const current = consentRef.current;
      if (!current?.progress || ev.model !== current.model) return;
      if (ev.state === "downloading") {
        updateConsent({ ...current, progress: { done: ev.done, total: ev.total } });
      } else if (ev.state === "ready") {
        updateConsent(null);
        void begin();
      } else if (ev.state === "error") {
        updateConsent(null);
        setErrorKey(voiceErrorKey(ev.key));
      } else {
        updateConsent(null);
      }
    })
      .then((u) => (disposed ? u() : (unlisten = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [begin]);

  // Automatic stop at the maximum length (`finished`).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onSpeechRecordingState((state) => {
      if (state === "finished" && phaseRef.current === "recording") void finish();
    })
      .then((u) => (disposed ? u() : (unlisten = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [finish]);

  // Provisional text: only while recording, only newer revisions.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onSpeechPartialText((ev) => {
      if (phaseRef.current !== "recording") return;
      if (ev.revision <= lastRevisionRef.current) return;
      lastRevisionRef.current = ev.revision;
      handlersRef.current.onPartial(ev.text);
    })
      .then((u) => (disposed ? u() : (unlisten = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Elapsed time while recording.
  useEffect(() => {
    if (phase !== "recording") return;
    const id = setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => clearInterval(id);
  }, [phase]);

  // Esc discards the recording.
  useEffect(() => {
    if (phase !== "recording") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [phase, cancel]);

  // Leaving the chat while recording discards the recording.
  useEffect(
    () => () => {
      if (phaseRef.current === "recording") {
        handlersRef.current.onCancel();
        void cancelVoiceRecording().catch(() => {});
      }
    },
    [],
  );

  return {
    phase,
    elapsed,
    errorKey,
    clearError: () => setErrorKey(null),
    consent,
    toggle,
    confirmDownload,
    dismissConsent,
  };
}
