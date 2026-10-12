import { settingsStore } from "./i18n";

/** Spec 0108 §5.4: „Spracheingabe“ ein/aus, im selben `tauri-plugin-store`
 * wie die übrigen UI-Einstellungen. Standard: an. */
const KEY = "voiceInputEnabled";

export async function loadVoiceInputEnabled(): Promise<boolean> {
  const store = await settingsStore();
  return (await store.get<unknown>(KEY)) !== false;
}

type Listener = (enabled: boolean) => void;
const listeners = new Set<Listener>();

/** Meldet Änderungen aus den Einstellungen an den (weiter gemounteten) Chat. */
export function subscribeVoiceInputEnabled(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export async function saveVoiceInputEnabled(enabled: boolean): Promise<void> {
  const store = await settingsStore();
  await store.set(KEY, enabled);
  await store.save();
  for (const l of listeners) l(enabled);
}

/** Übersetzungsschlüssel (`voice.errors.*`) für einen Fehlercode der
 * Spracheingabe; unbekannte Codes ergeben die allgemeine Meldung. */
export function voiceErrorKey(code: string | null): string {
  switch (code) {
    case "speech_no_device":
      return "voice.errors.noDevice";
    case "speech_permission_denied":
      return "voice.errors.permissionDenied";
    case "speech_transcription_failed":
      return "voice.errors.transcriptionFailed";
    case "speech_model_hash_mismatch":
      return "voice.errors.hashMismatch";
    case "speech_model_network":
    case "speech_model_http_status":
    case "speech_model_io":
      return "voice.errors.downloadFailed";
    default:
      return "voice.errors.generic";
  }
}

/** Hängt erkannten Text an den Entwurf an (Leerzeichen nur bei nicht leerem Entwurf). */
export function appendToDraft(draft: string, text: string): string {
  const addition = text.trim();
  if (!addition) return draft;
  if (draft.length === 0) return addition;
  return /\s$/.test(draft) ? draft + addition : `${draft} ${addition}`;
}

/** `m:ss` */
export function formatElapsed(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
