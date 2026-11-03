import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { KeyboardEvent } from "react";
import { listPromptHistory } from "../../api";
import {
  registerComposerDropHandler,
  resetComposerRegistryForTests,
  type ComposerAttachment,
} from "../../extensions/registry";
import { useChatComposer } from "./useChatComposer";

const dragState = vi.hoisted(() => ({
  handler: null as null | ((e: unknown) => void),
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (h: (e: unknown) => void) => {
      dragState.handler = h;
      return Promise.resolve(() => {
        dragState.handler = null;
      });
    },
  }),
}));
vi.mock("../../api", () => ({
  listPromptHistory: vi.fn(),
  commandErrorMessage: (e: unknown) => String(e),
}));

const attachment = (id: string): ComposerAttachment => ({
  id,
  name: `${id}.txt`,
  kind: "text",
  size: 10,
}) as ComposerAttachment;

function setup() {
  const input = document.createElement("textarea");
  const area = document.createElement("div");
  return renderHook(() =>
    useChatComposer("session-1", "server-1", { current: input }, { current: area }),
  );
}

function arrow(key: "ArrowUp" | "ArrowDown", value: string, pos: number) {
  const preventDefault = vi.fn();
  const currentTarget = { selectionStart: pos, selectionEnd: pos, value };
  return {
    event: { key, preventDefault, currentTarget } as unknown as KeyboardEvent<HTMLTextAreaElement>,
    preventDefault,
  };
}

describe("useChatComposer", () => {
  beforeEach(() => {
    vi.mocked(listPromptHistory).mockResolvedValue(["first", "second"]);
    resetComposerRegistryForTests();
  });
  afterEach(() => resetComposerRegistryForTests());

  it("prompt history up/down restores the typed draft", async () => {
    const { result } = setup();
    await waitFor(() => expect(listPromptHistory).toHaveBeenCalled());
    act(() => result.current.handleDraftChange("typed"));
    // Wait for the history to be loaded into the hook before navigating.
    await act(async () => {});
    act(() => result.current.handleInputKeyDown(arrow("ArrowUp", "typed", 0).event));
    expect(result.current.draft).toBe("second");
    act(() => result.current.handleInputKeyDown(arrow("ArrowUp", "second", 0).event));
    expect(result.current.draft).toBe("first");
    act(() => result.current.handleInputKeyDown(arrow("ArrowDown", "first", 5).event));
    expect(result.current.draft).toBe("second");
    act(() => result.current.handleInputKeyDown(arrow("ArrowDown", "second", 6).event));
    expect(result.current.draft).toBe("typed");
  });

  it("adds an attachment once and removes it again", () => {
    const { result } = setup();
    act(() => result.current.composerCtx.addAttachment(attachment("a")));
    act(() => result.current.composerCtx.addAttachment(attachment("a")));
    act(() => result.current.composerCtx.addAttachment(attachment("b")));
    expect(result.current.attachments.map((a) => a.id)).toEqual(["a", "b"]);
    act(() => result.current.composerCtx.removeAttachment("a"));
    expect(result.current.attachments.map((a) => a.id)).toEqual(["b"]);
  });

  it("routes a drop on the chat area to the registered drop handler", async () => {
    const handler = vi.fn();
    registerComposerDropHandler("test-drop", handler);
    const input = document.createElement("textarea");
    const area = document.createElement("div");
    area.getBoundingClientRect = () =>
      ({ left: 0, right: 100, top: 0, bottom: 100 }) as DOMRect;
    Object.defineProperty(area, "offsetParent", { get: () => document.body });
    renderHook(() => useChatComposer("session-1", "server-1", { current: input }, { current: area }));
    await waitFor(() => expect(dragState.handler).not.toBeNull());
    act(() => dragState.handler?.({ payload: { type: "drop", position: { x: 500, y: 500 } } }));
    expect(handler).not.toHaveBeenCalled();
    act(() => dragState.handler?.({ payload: { type: "drop", position: { x: 10, y: 10 } } }));
    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler.mock.calls[0][0]).toMatchObject({ sessionId: "session-1" });
  });
});
