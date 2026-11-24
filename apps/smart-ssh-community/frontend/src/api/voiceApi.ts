import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { EVENT } from "./eventNames";
import { listenTyped } from "./listenTyped";

// Spec 0108: Wrapper um die Befehle und Ereignisse der Spracheingabe. Eigene
// Datei, damit die Chat-Oberfläche sie nur bei einer Nutzeraktion ruft.

/** Model id as reported by `list_speech_models` (e.g. `base`, `small`, `parakeet_v3`). */
export type SpeechModelId = string;

/** Entry of `list_speech_models`: what the backend offers on this platform. */
export interface SpeechModelInfo {
  id: string;
  engine: "whisper" | "parakeet";
  total_size: number;
  source_host: string;
  source_repo: string;
}

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
export const listSpeechModels = () => invoke<SpeechModelInfo[]>("list_speech_models");
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
  listenTyped(EVENT.SPEECH_MODEL_STATUS, handler);

export const onSpeechRecordingState = (
  handler: (state: RecordingState) => void,
): Promise<UnlistenFn> =>
  listenTyped(EVENT.SPEECH_RECORDING_STATE, (payload) => handler(payload.state));

/** Provisional text of the running recording (Spec 0108 §5.7); revisions only grow. */
export interface SpeechPartialEvent {
  text: string;
  revision: number;
}

export const onSpeechPartialText = (
  handler: (event: SpeechPartialEvent) => void,
): Promise<UnlistenFn> =>
  listenTyped(EVENT.SPEECH_PARTIAL_TEXT, handler);

/** Download size in MiB (rounded up) for display; the backend reports bytes. */
export const speechModelSizeMib = (info: SpeechModelInfo): number =>
  Math.ceil(info.total_size / (1024 * 1024));

/** Display name of a model; Whisper models keep their plain id. */
export const SPEECH_MODEL_PARAKEET = "parakeet_v3";
