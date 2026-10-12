import { useEffect, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useVoiceInput } from "../useVoiceInput";
import { SPEECH_MODEL_SIZE_MIB, SPEECH_MODEL_SOURCE } from "../voiceApi";
import { formatElapsed } from "../voiceInputSettings";
import { Button, DialogFrame, Spinner } from "./ui";

const IS_MAC = typeof navigator !== "undefined" && navigator.platform.toLowerCase().includes("mac");

/** Spec 0108 §5.1: toggles recording with Ctrl+Shift+M (Cmd+Shift+M on macOS). */
export function isVoiceShortcut(e: KeyboardEvent, mac = IS_MAC): boolean {
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  return mod && e.shiftKey && !e.altKey && (e.code === "KeyM" || e.key.toLowerCase() === "m");
}

interface VoiceInputControlProps {
  onText: (text: string) => void;
  /** The shortcut only works while focus is inside this element. */
  panelRef: RefObject<HTMLElement | null>;
}

/** Microphone button next to the send button, plus consent dialog and error line. */
export function VoiceInputControl({ onText, panelRef }: VoiceInputControlProps) {
  const { t } = useTranslation();
  const voice = useVoiceInput(onText);
  const { toggle } = voice;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isVoiceShortcut(e)) return;
      if (!panelRef.current?.contains(document.activeElement)) return;
      e.preventDefault();
      void toggle();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [panelRef, toggle]);

  const recording = voice.phase === "recording";
  const transcribing = voice.phase === "transcribing";
  const label = recording
    ? t("voice.stop", { time: formatElapsed(voice.elapsed) })
    : transcribing
      ? t("voice.transcribing")
      : t("voice.start");
  const shortcut = IS_MAC ? "⌘⇧M" : "Ctrl+Shift+M";

  return (
    <>
      <button
        type="button"
        onClick={() => void toggle()}
        disabled={transcribing}
        aria-label={label}
        aria-pressed={recording}
        title={`${label} (${shortcut})`}
        data-voice-state={voice.phase}
        className={`inline-flex items-center justify-center gap-1.5 rounded border px-2.5 py-1.5 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 disabled:cursor-not-allowed disabled:opacity-70 ${
          recording
            ? "animate-pulse border-red-700 bg-red-900 text-red-100"
            : "border-slate-600 bg-slate-800 text-slate-100 hover:bg-slate-700"
        }`}
      >
        {transcribing ? <Spinner /> : <MicIcon />}
        {recording && <span className="font-mono text-xs">{formatElapsed(voice.elapsed)}</span>}
      </button>
      <span aria-live="polite" className="sr-only">
        {recording ? t("voice.recordingStatus") : transcribing ? t("voice.transcribing") : ""}
      </span>
      {voice.errorKey && (
        <p
          role="alert"
          className="basis-full rounded bg-red-950 px-3 py-2 text-xs text-red-300"
        >
          {t(voice.errorKey)}{" "}
          <button type="button" className="underline" onClick={voice.clearError}>
            {t("voice.dismiss")}
          </button>
        </p>
      )}
      {voice.consent && (
        <DialogFrame
          layer="app"
          title={t("voice.consent.title")}
          onClose={voice.dismissConsent}
          actions={
            <>
              <Button variant="secondary" onClick={voice.dismissConsent}>
                {t("voice.consent.cancel")}
              </Button>
              {!voice.consent.progress && (
                <Button variant="primary" onClick={() => void voice.confirmDownload()}>
                  {t("voice.consent.download")}
                </Button>
              )}
            </>
          }
        >
          <p className="text-sm text-slate-300">
            {t("voice.consent.body", {
              model: voice.consent.model,
              size: SPEECH_MODEL_SIZE_MIB[voice.consent.model],
              source: SPEECH_MODEL_SOURCE,
            })}
          </p>
          {voice.consent.progress && (
            <progress
              className="mt-3 w-full"
              aria-label={t("voice.consent.progress")}
              max={voice.consent.progress.total || undefined}
              value={voice.consent.progress.total ? voice.consent.progress.done : undefined}
            />
          )}
        </DialogFrame>
      )}
    </>
  );
}

function MicIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
    </svg>
  );
}
