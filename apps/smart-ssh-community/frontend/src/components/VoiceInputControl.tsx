import { useEffect, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useVoiceInput, type VoiceHandlers, type VoicePhase } from "../hooks/useVoiceInput";
import { isMacLike } from "../lib/platform";
import {
  listSpeechModels,
  SPEECH_MODEL_PARAKEET,
  speechModelSizeMib,
  type SpeechModelInfo,
} from "../api/voiceApi";
import { formatElapsed } from "../settings/voiceInputSettings";
import { Button, DialogFrame, Spinner, Icon } from "./ui";

/** Product name, not translated. */
const PARAKEET_DISPLAY_NAME = "Parakeet v3 (NVIDIA)";

const IS_MAC = isMacLike();

/** Spec 0108 §5.1: toggles recording with Ctrl+Shift+M (Cmd+Shift+M on macOS). */
export function isVoiceShortcut(e: KeyboardEvent, mac = IS_MAC): boolean {
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  return mod && e.shiftKey && !e.altKey && (e.code === "KeyM" || e.key.toLowerCase() === "m");
}

interface VoiceInputControlProps {
  handlers: VoiceHandlers;
  /** Reports the phase so the chat input can lock while it is not `idle`. */
  onPhaseChange: (phase: VoicePhase) => void;
  /** The shortcut only works while focus is inside this element. */
  panelRef: RefObject<HTMLElement | null>;
}

/** Microphone button next to the send button, plus consent dialog and error line. */
export function VoiceInputControl({ handlers, onPhaseChange, panelRef }: VoiceInputControlProps) {
  const { t } = useTranslation();
  const voice = useVoiceInput(handlers);
  const { toggle } = voice;
  const { phase } = voice;
  useEffect(() => {
    onPhaseChange(phase);
  }, [phase, onPhaseChange]);

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
      <Button variant="bare"
        type="button"
        onClick={() => void toggle()}
        disabled={transcribing}
        aria-label={label}
        aria-pressed={recording}
        title={`${label} (${shortcut})`}
        data-voice-state={voice.phase}
        className={`inline-flex items-center justify-center gap-1.5 rounded border px-2.5 py-1.5 text-sm focus-ring-flush disabled:cursor-not-allowed disabled:opacity-70 ${
          recording
            ? "animate-pulse border-danger-border-strong bg-danger-subtle-hover text-danger-fg-faint"
            : "border-transparent bg-transparent text-fg-muted hover:bg-border hover:text-fg-bright"
        }`}
      >
        {transcribing ? <Spinner /> : <Icon name="mic" />}
        {recording && <span className="font-mono text-xs">{formatElapsed(voice.elapsed)}</span>}
      </Button>
      <span aria-live="polite" className="sr-only">
        {recording ? t("voice.recordingStatus") : transcribing ? t("voice.transcribing") : ""}
      </span>
      {voice.errorKey && (
        <p
          role="alert"
          className="basis-full rounded bg-danger-subtle px-3 py-2 text-xs text-danger-fg-soft"
        >
          {t(voice.errorKey)}{" "}
          <Button variant="bare" type="button" className="underline" onClick={voice.clearError}>
            {t("voice.dismiss")}
          </Button>
        </p>
      )}
      {voice.consent && (
        <DialogFrame
          layer="app"
          title={t("voice.consent.title")}
          onClose={voice.closeConsent}
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
          <ConsentText model={voice.consent.model} />
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

/** Model, size and source (and license note for Parakeet) from the backend list. */
function ConsentText({ model }: { model: string }) {
  const { t } = useTranslation();
  const [info, setInfo] = useState<SpeechModelInfo | null>(null);
  useEffect(() => {
    let live = true;
    listSpeechModels()
      .then((list) => live && setInfo(list.find((m) => m.id === model) ?? null))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [model]);
  if (!info) return null;
  const parakeet = info.id === SPEECH_MODEL_PARAKEET;
  return (
    <>
      <p className="text-sm text-fg-muted">
        {t("voice.consent.body", {
          model: parakeet ? PARAKEET_DISPLAY_NAME : info.id,
          size: speechModelSizeMib(info),
          source: parakeet ? `${info.source_host}/${info.source_repo}` : info.source_host,
        })}
      </p>
      {parakeet && <p className="mt-2 text-sm text-fg-muted">{t("voice.consent.parakeetLicense", { license: "CC-BY-4.0, NVIDIA" })}</p>}
    </>
  );
}
