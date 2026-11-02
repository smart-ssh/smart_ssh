import { type Dispatch, type RefObject, type SetStateAction, useEffect, useMemo, useRef, useState } from "react";
import { appendToDraft, loadVoiceInputEnabled, subscribeVoiceInputEnabled } from "../../voiceInputSettings";
import type { VoiceHandlers, VoicePhase } from "../../useVoiceInput";

/**
 * Spec 0108: voice input state of the chat composer. Recognised text goes
 * into the draft via `setDraft`, never directly to the AI.
 */
export function useVoiceComposer(
  draft: string,
  setDraft: Dispatch<SetStateAction<string>>,
  inputRef: RefObject<HTMLTextAreaElement | null>,
) {
  const [voiceEnabled, setVoiceEnabled] = useState(true);
  const [voicePhase, setVoicePhase] = useState<VoicePhase>("idle");
  const [voiceFocusPending, setVoiceFocusPending] = useState(false);
  const voiceBaseRef = useRef<string | null>(null);
  useEffect(() => {
    loadVoiceInputEnabled()
      .then(setVoiceEnabled)
      .catch(() => {});
    return subscribeVoiceInputEnabled(setVoiceEnabled);
  }, []);
  const draftRef = useRef("");
  draftRef.current = draft;
  // Spec 0108 §5.2: live text. The draft before Start is kept; provisional
  // text replaces everything after it, so no merging with edits is needed
  // (the input is read-only meanwhile).
  const voiceHandlers = useMemo<VoiceHandlers>(() => {
    const showAfterBase = (text: string) => {
      const base = voiceBaseRef.current;
      if (base === null) return;
      setDraft(appendToDraft(base, text));
    };
    return {
      onStart: () => {
        voiceBaseRef.current = draftRef.current;
      },
      onPartial: showAfterBase,
      onFinal: (text) => {
        showAfterBase(text);
        voiceBaseRef.current = null;
        setVoiceFocusPending(true);
      },
      onFailed: () => {
        voiceBaseRef.current = null;
        setVoiceFocusPending(true);
      },
      onCancel: () => {
        if (voiceBaseRef.current !== null) setDraft(voiceBaseRef.current);
        voiceBaseRef.current = null;
      },
    };
  }, [setDraft]);
  const voiceLocked = voicePhase !== "idle";
  // Once the input is editable again: focus it, caret at the end.
  useEffect(() => {
    if (!voiceFocusPending || voiceLocked) return;
    setVoiceFocusPending(false);
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
  }, [voiceFocusPending, voiceLocked, inputRef]);

  return { voiceEnabled, voicePhase, setVoicePhase, voiceLocked, voiceHandlers };
}
