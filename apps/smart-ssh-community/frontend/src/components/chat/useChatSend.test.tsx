import { act, renderHook, waitFor } from "@testing-library/react";
import type { FormEvent } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { continueTruncatedResponse, retryChatTurn, sendChatMessage } from "../../api";
import { useChatSend } from "./useChatSend";

vi.mock("../../api", () => ({
  sendChatMessage: vi.fn(),
  continueTruncatedResponse: vi.fn(),
  retryChatTurn: vi.fn(),
  stopAutoContinuation: vi.fn(),
  startNewChat: vi.fn(),
  commandErrorCode: () => null,
  commandErrorMessage: (e: unknown) => String(e),
}));

function setup(draft = "hello") {
  const composer = {
    draft,
    attachments: [{ id: "a1", name: "f.txt", kind: "file" }],
    setDraft: vi.fn(),
    setAttachments: vi.fn(),
    setComposerError: vi.fn(),
    resetHistoryNav: vi.fn(),
    reloadPromptHistory: vi.fn(),
  };
  const dispatch = vi.fn();
  const hook = renderHook(() =>
    useChatSend("s1", {
      composer: composer as never,
      dispatch,
      voiceLocked: false,
      scrollToBottom: vi.fn(),
      autoContinuing: false,
      confirmationOpen: false,
    }),
  );
  return { composer, dispatch, ...hook };
}

describe("useChatSend", () => {
  beforeEach(() => {
    vi.mocked(sendChatMessage).mockReset().mockResolvedValue(undefined);
    vi.mocked(continueTruncatedResponse).mockReset().mockResolvedValue(undefined);
    vi.mocked(retryChatTurn).mockReset().mockResolvedValue(undefined);
  });

  it("submit sends draft and attachment ids and clears the draft", async () => {
    const { result, composer } = setup();
    await act(() => result.current.handleSubmit({ preventDefault() {} } as FormEvent));
    expect(sendChatMessage).toHaveBeenCalledWith("s1", "hello", ["a1"]);
    expect(composer.setDraft).toHaveBeenCalledWith("");
    expect(composer.setAttachments).toHaveBeenCalledWith([]);
    await waitFor(() => expect(result.current.sending).toBe(false));
  });

  it("an empty draft sends nothing", async () => {
    const { result } = setup("  ");
    await act(() => result.current.handleSubmit({ preventDefault() {} } as FormEvent));
    expect(sendChatMessage).not.toHaveBeenCalled();
  });

  it("continue and retry call the matching API", async () => {
    const { result } = setup();
    await act(() => result.current.handleContinueTruncated("i1"));
    expect(continueTruncatedResponse).toHaveBeenCalledWith("s1");
    await act(() => result.current.handleRetryTurn("i2"));
    expect(retryChatTurn).toHaveBeenCalledWith("s1");
  });
});
