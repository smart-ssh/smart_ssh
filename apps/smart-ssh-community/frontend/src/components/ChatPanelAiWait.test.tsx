// Issue #694: AI wait line and named timeout errors.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getChatHistory, sendChatMessage } from "../api";
import { onChatAiRequestStarted, onChatError, onChatTextDelta } from "../events";
import type { ChatAiRequestStartedEvent, ChatErrorEvent } from "../types";
import { testI18n } from "../testI18n";
import { ChatPanel } from "./ChatPanel";

if (!Element.prototype.scrollTo) {
  Element.prototype.scrollTo = () => {};
}

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  acceptAndCreateRule: vi.fn(),
  cancelRunningCommand: vi.fn(),
  retryChatTurn: vi.fn(() => Promise.resolve()),
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
  startNewChat: vi.fn(() => Promise.resolve("new-chat-id")),
  stopAutoContinuation: vi.fn(() => Promise.resolve()),
  suggestRulePatterns: vi.fn(),
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
  onChatContextUsage: vi.fn(() => Promise.resolve(() => {})),
  onChatContextCompacted: vi.fn(() => Promise.resolve(() => {})),
  onChatAiRetrying: vi.fn(() => Promise.resolve(() => {})),
  onChatAiRequestStarted: vi.fn(() => Promise.resolve(() => {})),
  onChatQueuedMessagesSent: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseCancelled: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseEmpty: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseRefused: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseTruncated: vi.fn(() => Promise.resolve(() => {})),
  onChatTextDelta: vi.fn(() => Promise.resolve(() => {})),
  onChatWebActivity: vi.fn(() => Promise.resolve(() => {})),
  onChatInjectionCheck: vi.fn(() => Promise.resolve(() => {})),
  onChatWebResearchUnavailable: vi.fn(() => Promise.resolve(() => {})),
  onRiskAssessmentUpdated: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

function handlerOf<T>(listener: unknown): (event: T) => void {
  const calls = vi.mocked(listener as (h: (event: T) => void) => unknown).mock.calls;
  return calls[calls.length - 1][0];
}

async function renderSending() {
  vi.mocked(getChatHistory).mockResolvedValue([
    { type: "text", role: "user", text: "check the disk" },
  ] as never);
  vi.mocked(sendChatMessage).mockImplementation(() => new Promise(() => {}));
  render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
    </I18nextProvider>,
  );
  await screen.findByText("check the disk");
  await waitFor(() => expect(onChatAiRequestStarted).toHaveBeenCalled());
  const field = screen.getByPlaceholderText("Frage stellen oder Kommando beschreiben …");
  fireEvent.change(field, { target: { value: "hallo" } });
  fireEvent.keyDown(field, { key: "Enter" });
  await waitFor(() => expect(sendChatMessage).toHaveBeenCalled());
}

describe("ChatPanel AI wait line (issue #694)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows provider and counting seconds, with Stop, until the first content", async () => {
    await renderSending();
    act(() =>
      handlerOf<ChatAiRequestStartedEvent>(onChatAiRequestStarted)({
        sessionId: "session-1",
        providerName: "Claude",
      }),
    );
    const line = screen.getByTestId("chat-ai-waiting");
    expect(line.textContent).toContain("Claude");
    expect(line.textContent).toContain("0 s");
    expect(screen.getByRole("button", { name: "Stopp" })).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(4000);
    });
    expect(screen.getByTestId("chat-ai-waiting").textContent).toContain("4 s");
    act(() =>
      handlerOf<{ sessionId: string; delta: string }>(onChatTextDelta)({
        sessionId: "session-1",
        delta: "ok",
      }),
    );
    expect(screen.queryByTestId("chat-ai-waiting")).toBeNull();
  });

  it("clears the wait line when the turn ends and does not show it stale on the next send", async () => {
    let finish: () => void = () => {};
    vi.mocked(getChatHistory).mockResolvedValue([
      { type: "text", role: "user", text: "check the disk" },
    ] as never);
    vi.mocked(sendChatMessage).mockImplementationOnce(
      () => new Promise<void>((resolve) => (finish = resolve)),
    );
    render(
      <I18nextProvider i18n={testI18n}>
        <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
      </I18nextProvider>,
    );
    await screen.findByText("check the disk");
    await waitFor(() => expect(onChatAiRequestStarted).toHaveBeenCalled());
    const field = screen.getByPlaceholderText("Frage stellen oder Kommando beschreiben …");
    fireEvent.change(field, { target: { value: "hallo" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(sendChatMessage).toHaveBeenCalled());
    act(() =>
      handlerOf<ChatAiRequestStartedEvent>(onChatAiRequestStarted)({
        sessionId: "session-1",
        providerName: "Claude",
      }),
    );
    expect(screen.getByTestId("chat-ai-waiting")).toBeInTheDocument();
    await act(async () => {
      finish();
    });
    expect(screen.queryByTestId("chat-ai-waiting")).toBeNull();
    vi.mocked(sendChatMessage).mockImplementation(() => new Promise(() => {}));
    fireEvent.change(field, { target: { value: "again" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(sendChatMessage).toHaveBeenCalledTimes(2));
    expect(screen.queryByTestId("chat-ai-waiting")).toBeNull();
  });

  it("names the provider on a timeout and offers Try again", async () => {
    vi.mocked(getChatHistory).mockResolvedValue([
      { type: "text", role: "user", text: "check the disk" },
    ] as never);
    render(
      <I18nextProvider i18n={testI18n}>
        <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
      </I18nextProvider>,
    );
    await screen.findByText("check the disk");
    await waitFor(() => expect(onChatError).toHaveBeenCalled());
    act(() =>
      handlerOf<ChatErrorEvent>(onChatError)({
        sessionId: "session-1",
        message: "x",
        code: "AI_TIMEOUT",
        providerName: "Claude",
        timeoutSecs: 120,
      }),
    );
    expect(screen.getByText(/Claude/).textContent).toContain("120 s");
    expect(screen.getByRole("button", { name: "Erneut versuchen" })).toBeInTheDocument();
  });

  it("names the provider on the total deadline and offers Try again", async () => {
    vi.mocked(getChatHistory).mockResolvedValue([
      { type: "text", role: "user", text: "check the disk" },
    ] as never);
    render(
      <I18nextProvider i18n={testI18n}>
        <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
      </I18nextProvider>,
    );
    await screen.findByText("check the disk");
    await waitFor(() => expect(onChatError).toHaveBeenCalled());
    act(() =>
      handlerOf<ChatErrorEvent>(onChatError)({
        sessionId: "session-1",
        message: "x",
        code: "AI_TOTAL_DEADLINE",
        providerName: "Claude",
        timeoutSecs: 600,
      }),
    );
    expect(screen.getByText(/Claude/).textContent).toContain("10 Minuten");
    expect(screen.getByRole("button", { name: "Erneut versuchen" })).toBeInTheDocument();
  });
});
