// Issue #471 (Spec 0038, Spec 0110): Composer-Erweiterungspunkt, Anhang-Chips,
// Drop-Routing und Anhänge im fortgesetzten Verlauf.
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getChatHistory, sendChatMessage } from "../api";
import {
  registerComposerAction,
  registerComposerDropHandler,
  unregisterComposerDropHandler,
  resetComposerRegistryForTests,
} from "../extensions/registry";
import { testI18n } from "../testI18n";
import { ChatPanel } from "./ChatPanel";

if (!Element.prototype.scrollTo) {
  Element.prototype.scrollTo = () => {};
}

const dragState = vi.hoisted(() => ({
  handler: null as null | ((e: unknown) => void),
  calls: 0,
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (h: (e: unknown) => void) => {
      dragState.calls += 1;
      dragState.handler = h;
      return Promise.resolve(() => {
        dragState.handler = null;
      });
    },
  }),
}));

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
  acceptAndCreateRule: vi.fn(),
  cancelRunningCommand: vi.fn(),
  continueTruncatedResponse: vi.fn(() => Promise.resolve()),
  exportDocument: vi.fn(),
  getChatHistory: vi.fn(() => Promise.resolve([])),
  listAiProviders: vi.fn(() =>
    Promise.resolve([
      {
        id: "p1",
        providerType: "anthropic",
        displayName: "Claude",
        baseUrl: null,
        model: "claude",
        supportsNativeToolCalling: true,
        isActive: true,
        extraHeaders: [],
        attestationUrl: null,
      },
    ]),
  ),
  listPromptHistory: vi.fn(() => Promise.resolve([])),
  respondToAction: vi.fn(() => Promise.resolve()),
  sendChatMessage: vi.fn(() => Promise.resolve()),
  stopAutoContinuation: vi.fn(() => Promise.resolve()),
  explainConfirmation: vi.fn(() => Promise.resolve({ segments: [], suggestions: [] })),
  takeChatContentIntoNote: vi.fn(),
}));

vi.mock("../events", () => ({
  onActionDecisionEscalated: vi.fn(() => Promise.resolve(() => {})),
  onAiBudgetWaiting: vi.fn(() => Promise.resolve(() => {})),
  onChatActionProposed: vi.fn(() => Promise.resolve(() => {})),
  onChatActionOutput: vi.fn(() => Promise.resolve(() => {})),
  onChatActionWaitingInput: vi.fn(() => Promise.resolve(() => {})),
  onChatActionResult: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationLimitReached: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationStarted: vi.fn(() => Promise.resolve(() => {})),
  onChatDocumentGenerated: vi.fn(() => Promise.resolve(() => {})),
  onChatQuestionAsked: vi.fn(() => Promise.resolve(() => {})),
  onChatError: vi.fn(() => Promise.resolve(() => {})),
  onChatQueuedMessagesSent: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseCancelled: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseEmpty: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseTruncated: vi.fn(() => Promise.resolve(() => {})),
  onChatTextDelta: vi.fn(() => Promise.resolve(() => {})),
  onChatWebActivity: vi.fn(() => Promise.resolve(() => {})),
  onChatWebResearchUnavailable: vi.fn(() => Promise.resolve(() => {})),
  onRiskAssessmentUpdated: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() =>
    Promise.resolve({ enabled: false, providerId: null }),
  ),
}));

function renderChatPanel() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel
        sessionId="session-1"
        serverId="server-1"
        onActionSettled={vi.fn()}
      />
    </I18nextProvider>,
  );
}

function registerTestAction() {
  registerComposerAction({
    id: "test-attach",
    label: "Attach file",
    icon: <span>+</span>,
    onInvoke: (ctx) => {
      ctx.addAttachment({
        id: "a1",
        name: "notes.txt",
        size: 2048,
        kind: "text",
      });
      ctx.addAttachment({
        id: "a2",
        name: "key.pem",
        size: 100,
        kind: "text",
        unredactedHint: true,
      });
    },
  });
}

describe("ChatPanel composer extension slot (Issue #471)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    dragState.calls = 0;
    dragState.handler = null;
    resetComposerRegistryForTests();
  });
  afterEach(() => resetComposerRegistryForTests());

  it("renders only the send button and no chip list without a registered action", async () => {
    renderChatPanel();
    await screen.findByPlaceholderText(
      "Frage stellen oder Kommando beschreiben …",
    );
    expect(
      screen.getAllByRole("button").map((b) => b.textContent),
    ).not.toContain("+");
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // Without a drop handler the chat area does not listen for drops at all.
    expect(dragState.calls).toBe(0);
  });

  it("renders registered actions as labelled buttons and adds/removes chips", async () => {
    registerTestAction();
    renderChatPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Attach file" }));
    expect(screen.getByText(/notes\.txt/)).toBeInTheDocument();
    expect(screen.getByText(/key\.pem/)).toBeInTheDocument();
    // Only the chip with `unredactedHint` shows the hint.
    expect(screen.getAllByText("unredigiert gesendet")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "notes.txt entfernen" }));
    expect(screen.queryByText(/notes\.txt/)).not.toBeInTheDocument();
    expect(screen.getByText(/key\.pem/)).toBeInTheDocument();
  });

  it("passes the attachment ids on send and clears the chips", async () => {
    registerTestAction();
    renderChatPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Attach file" }));
    fireEvent.change(
      screen.getByPlaceholderText("Frage stellen oder Kommando beschreiben …"),
      {
        target: { value: "look at this" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Senden" }));
    await waitFor(() =>
      expect(sendChatMessage).toHaveBeenCalledWith(
        "session-1",
        "look at this",
        ["a1", "a2"],
      ),
    );
    await waitFor(() =>
      expect(screen.queryByRole("list")).not.toBeInTheDocument(),
    );
    // The sent message shows its attachments.
    expect(
      screen.getByText(/Angehängt: notes\.txt \(Text\), key\.pem \(Text\)/),
    ).toBeInTheDocument();
  });

  it("sends without attachment ids when no chip is pending", async () => {
    renderChatPanel();
    fireEvent.change(
      await screen.findByPlaceholderText(
        "Frage stellen oder Kommando beschreiben …",
      ),
      {
        target: { value: "hi" },
      },
    );
    fireEvent.click(screen.getByRole("button", { name: "Senden" }));
    await waitFor(() =>
      expect(sendChatMessage).toHaveBeenCalledWith(
        "session-1",
        "hi",
      ),
    );
  });

  it("shows a backend error at the composer and keeps text and chips", async () => {
    registerTestAction();
    vi.mocked(sendChatMessage).mockRejectedValueOnce("too many attachments");
    renderChatPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Attach file" }));
    const input = screen.getByPlaceholderText(
      "Frage stellen oder Kommando beschreiben …",
    );
    fireEvent.change(input, { target: { value: "look" } });
    fireEvent.click(screen.getByRole("button", { name: "Senden" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "too many attachments",
    );
    expect(screen.getByText(/notes\.txt/)).toBeInTheDocument();
    expect(input).toHaveValue("look");
  });

  it("lists extension parts of a resumed message by label and kind, never content", async () => {
    vi.mocked(getChatHistory).mockResolvedValueOnce([
      { type: "text", role: "user", text: "see parts" },
      {
        type: "extensionParts",
        role: "user",
        items: [{ label: "report.pdf", media: "pdf" }],
      },
      {
        type: "extensionParts",
        role: "user",
        items: [{ label: "log.txt", media: "text" }],
      },
    ]);
    renderChatPanel();
    expect(
      await screen.findByText("Angehängt: report.pdf (PDF)"),
    ).toBeInTheDocument();
    expect(screen.getByText("Angehängt: log.txt (Text)")).toBeInTheDocument();
  });

  it("routes a native drop on the chat area to the registered handler", async () => {
    const handler = vi.fn();
    registerComposerDropHandler("test-drop", handler);
    renderChatPanel();
    await waitFor(() => expect(dragState.handler).not.toBeNull());
    // jsdom has no layout: give the chat area a rect and an offsetParent.
    const area = document.querySelector(".flex.h-full.flex-col") as HTMLElement;
    area.getBoundingClientRect = () =>
      ({ left: 0, right: 100, top: 0, bottom: 100 }) as DOMRect;
    Object.defineProperty(area, "offsetParent", { get: () => document.body });
    act(() =>
      dragState.handler?.({
        payload: { type: "drop", position: { x: 500, y: 500 } },
      }),
    );
    expect(handler).not.toHaveBeenCalled();
    act(() =>
      dragState.handler?.({
        payload: { type: "drop", position: { x: 10, y: 10 } },
      }),
    );
    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler.mock.calls[0][0]).toMatchObject({ sessionId: "session-1" });
  });

  describe("drop zone by pointer position", () => {
    function prepare() {
      const area = document.querySelector(".flex.h-full.flex-col") as HTMLElement;
      area.getBoundingClientRect = () =>
        ({ left: 0, right: 100, top: 0, bottom: 100 }) as DOMRect;
      Object.defineProperty(area, "offsetParent", { get: () => document.body });
    }
    afterEach(() => {
      Object.defineProperty(window, "devicePixelRatio", { value: 1, configurable: true });
    });

    it("highlights only while a handler is registered and the pointer is over the chat", async () => {
      registerComposerDropHandler("test-drop", vi.fn());
      renderChatPanel();
      await waitFor(() => expect(dragState.handler).not.toBeNull());
      prepare();
      act(() => dragState.handler?.({ payload: { type: "over", position: { x: 10, y: 10 } } }));
      expect(screen.getByTestId("chat-drop-highlight")).toBeInTheDocument();
      act(() => dragState.handler?.({ payload: { type: "over", position: { x: 500, y: 500 } } }));
      expect(screen.queryByTestId("chat-drop-highlight")).toBeNull();
      act(() => dragState.handler?.({ payload: { type: "over", position: { x: 10, y: 10 } } }));
      act(() => dragState.handler?.({ payload: { type: "leave" } }));
      expect(screen.queryByTestId("chat-drop-highlight")).toBeNull();
      act(() => dragState.handler?.({ payload: { type: "over", position: { x: 10, y: 10 } } }));
      act(() => dragState.handler?.({ payload: { type: "drop", position: { x: 10, y: 10 } } }));
      expect(screen.queryByTestId("chat-drop-highlight")).toBeNull();
    });

    it("shows nothing and listens to nothing without a handler", async () => {
      renderChatPanel();
      await new Promise((r) => setTimeout(r, 0));
      expect(dragState.handler).toBeNull();
      expect(screen.queryByTestId("chat-drop-highlight")).toBeNull();
    });

    it("unregisterComposerDropHandler stops listening and highlight; unknown id is a no-op", async () => {
      const handler = vi.fn();
      registerComposerDropHandler("test-drop", handler);
      renderChatPanel();
      await waitFor(() => expect(dragState.handler).not.toBeNull());
      prepare();
      const before = dragState.handler;
      act(() => dragState.handler?.({ payload: { type: "over", position: { x: 10, y: 10 } } }));
      expect(screen.getByTestId("chat-drop-highlight")).toBeInTheDocument();
      act(() => unregisterComposerDropHandler("unknown"));
      expect(dragState.handler).toBe(before);
      act(() => unregisterComposerDropHandler("test-drop"));
      expect(dragState.handler).toBeNull();
      expect(screen.queryByTestId("chat-drop-highlight")).toBeNull();
      expect(handler).not.toHaveBeenCalled();
    });

    it("converts positions with devicePixelRatio", async () => {
      Object.defineProperty(window, "devicePixelRatio", { value: 2, configurable: true });
      const handler = vi.fn();
      registerComposerDropHandler("test-drop", handler);
      renderChatPanel();
      await waitFor(() => expect(dragState.handler).not.toBeNull());
      prepare();
      act(() => dragState.handler?.({ payload: { type: "drop", position: { x: 150, y: 150 } } }));
      expect(handler).toHaveBeenCalledTimes(1);
      act(() => dragState.handler?.({ payload: { type: "drop", position: { x: 250, y: 250 } } }));
      expect(handler).toHaveBeenCalledTimes(1);
    });
  });
});
