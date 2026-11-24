import { act, renderHook, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { useVoiceComposer } from "./useVoiceComposer";

vi.mock("../../settings/voiceInputSettings", async (importActual) => ({
  ...(await importActual<typeof import("../../settings/voiceInputSettings")>()),
  loadVoiceInputEnabled: vi.fn(() => Promise.resolve(true)),
  subscribeVoiceInputEnabled: vi.fn(() => () => {}),
}));

function setup(initial: string, input: HTMLTextAreaElement) {
  return renderHook(() => {
    const [draft, setDraft] = useState(initial);
    const voice = useVoiceComposer(draft, setDraft, { current: input });
    return { draft, ...voice };
  });
}

describe("useVoiceComposer", () => {
  it("inserts recognised text after the existing draft and restores it on cancel", async () => {
    const { result } = setup("hello", document.createElement("textarea"));
    await waitFor(() => expect(result.current.voiceEnabled).toBe(true));
    act(() => result.current.voiceHandlers.onStart());
    act(() => result.current.voiceHandlers.onPartial("wor"));
    expect(result.current.draft).toBe("hello wor");
    act(() => result.current.voiceHandlers.onPartial("world"));
    expect(result.current.draft).toBe("hello world");
    act(() => result.current.voiceHandlers.onCancel());
    expect(result.current.draft).toBe("hello");
  });

  it("final text stays in the draft and focuses the input with the caret at the end", () => {
    const input = document.createElement("textarea");
    document.body.appendChild(input);
    const { result } = setup("a", input);
    act(() => result.current.voiceHandlers.onStart());
    act(() => result.current.voiceHandlers.onFinal("b"));
    input.value = result.current.draft;
    expect(result.current.draft).toBe("a b");
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(3);
    input.remove();
  });
});
