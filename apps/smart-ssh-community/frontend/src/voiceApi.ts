import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Spec 0108: Wrapper um die Befehle und Ereignisse der Spracheingabe. Eigene
// Datei, damit die Chat-Oberfläche sie nur bei einer Nutzeraktion ruft.

export type SpeechModelId = "base" | "small";
export const SPEECH_MODELS: readonly SpeechModelId[] = ["base", "small"];

export type SpeechModelStatus =
  | { state: "not_downloaded" }
  | { state: "downloading"; done: number; total: number }
  | { state: "ready" }
  | { state: "error"; key: string };

export type SpeechModelStatusEvent = SpeechModelStatus & { model: SpeechModelId };

export type RecordingState = "recording" | "transcribing" | "idle" | "finished";

export const getSelectedSpeechModel = () => invoke<SpeechModelId>("get_selected_speech_model");
export const setSelectedSpeechModel = (model: SpeechModelId) =>
  invoke<void>("set_selected_speech_model", { model });
export const getSpeechModelStatus = (model: SpeechModelId) =>
  invoke<SpeechModelStatusEvent>("get_speech_model_status", { model });
export const startSpeechModelDownload = (model: SpeechModelId) =>
  invoke<void>("start_speech_model_download", { model });
export const cancelSpeechModelDownload = (model: SpeechModelId) =>
  invoke<void>("cancel_speech_model_download", { model });
export const deleteSpeechModel = (model: SpeechModelId) =>
  invoke<void>("delete_speech_model", { model });
export const getSpeechLanguage = () => invoke<string>("get_speech_language");
export const setSpeechLanguage = (language: string) =>
  invoke<void>("set_speech_language", { language });
export const startVoiceRecording = () => invoke<void>("start_voice_recording");
export const stopVoiceRecording = () => invoke<string>("stop_voice_recording");
export const cancelVoiceRecording = () => invoke<void>("cancel_voice_recording");

export const onSpeechModelStatus = (
  handler: (event: SpeechModelStatusEvent) => void,
): Promise<UnlistenFn> =>
  listen<SpeechModelStatusEvent>("speech-model-status", (e) => handler(e.payload));

export const onSpeechRecordingState = (
  handler: (state: RecordingState) => void,
): Promise<UnlistenFn> =>
  listen<{ state: RecordingState }>("speech-recording-state", (e) => handler(e.payload.state));

/** Approximate download sizes (Spec 0108 §2.1), for the consent dialog and settings. */
export const SPEECH_MODEL_SIZE_MIB: Record<SpeechModelId, number> = { base: 142, small: 466 };
export const SPEECH_MODEL_SOURCE = "huggingface.co";
