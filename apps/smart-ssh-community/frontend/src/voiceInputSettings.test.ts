import { describe, expect, it } from "vitest";
import { appendToDraft, formatElapsed, voiceErrorKey } from "./voiceInputSettings";

describe("appendToDraft", () => {
  it("returns the text for an empty draft", () => {
    expect(appendToDraft("", " hallo ")).toBe("hallo");
  });
  it("separates by one space from a non-empty draft", () => {
    expect(appendToDraft("ls", "und mehr")).toBe("ls und mehr");
    expect(appendToDraft("ls ", "und mehr")).toBe("ls und mehr");
  });
  it("leaves the draft alone for empty text", () => {
    expect(appendToDraft("ls", "  ")).toBe("ls");
  });
});

describe("voiceErrorKey / formatElapsed", () => {
  it("maps backend codes to messages", () => {
    expect(voiceErrorKey("speech_no_device")).toBe("voice.errors.noDevice");
    expect(voiceErrorKey("speech_permission_denied")).toBe("voice.errors.permissionDenied");
    expect(voiceErrorKey("speech_transcription_failed")).toBe("voice.errors.transcriptionFailed");
    expect(voiceErrorKey("speech_model_hash_mismatch")).toBe("voice.errors.hashMismatch");
    expect(voiceErrorKey("speech_model_network")).toBe("voice.errors.downloadFailed");
    expect(voiceErrorKey("whatever")).toBe("voice.errors.generic");
    expect(voiceErrorKey(null)).toBe("voice.errors.generic");
  });
  it("formats m:ss", () => {
    expect(formatElapsed(65)).toBe("1:05");
  });
});
