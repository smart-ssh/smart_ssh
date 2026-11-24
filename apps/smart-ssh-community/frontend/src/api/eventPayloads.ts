// Payload type of every backend event, by event name (issue #920).
// The core events come from the Rust payload structs (`EventPayloads` in
// `generated/ipc.ts`); the events that `app-shell` emits directly are listed
// here by hand. `npm run check-events` fails when an `EVENT` constant has no
// entry in either place.
import type { EventPayloads as GeneratedEventPayloads } from "../generated/ipc";
import type { SpeechModelStatusEvent, SpeechPartialEvent, RecordingState } from "./voiceApi";
import type { StartupPromptRequest } from "./types";

export type EventPayloads = GeneratedEventPayloads & {
  "speech-model-status": SpeechModelStatusEvent;
  "speech-recording-state": { state: RecordingState };
  "speech-partial-text": SpeechPartialEvent;
  "startup:prompt": StartupPromptRequest;
  "startup:unlocked": null;
};
