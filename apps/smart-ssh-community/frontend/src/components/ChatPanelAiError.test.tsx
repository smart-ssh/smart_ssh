// Issue #600: AI error card (Details, Try again) and retry countdown.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { retryChatTurn, getChatHistory } from "../api";
import { onChatAiRetrying, onChatError, onChatTextDelta } from "../events";
import type { ChatAiRetryingEvent, ChatErrorEvent } from "../types";
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
  listAiProviders: vi.fn(() => Promise.resolve([])),
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
  onChatResponseTruncated: vi.fn(() => Promise.resolve(() => {})),
  onChatTextDelta: vi.fn(() => Promise.resolve(() => {})),
  onChatWebActivity: vi.fn(() => Promise.resolve(() => {})),
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

async function renderWithUserTurn() {
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
}

const overloaded: ChatErrorEvent = {
  sessionId: "session-1",
  message: "KI-Provider ist überlastet",
  code: "AI_PROVIDER_OVERLOADED",
  providerErrorType: "overloaded_error",
  httpStatus: 529,
  providerMessage: "Overloaded",
};

describe("ChatPanel AI error card (issue #600)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows text, collapsed details with code/type/status/message, and Try again", async () => {
    await renderWithUserTurn();
    act(() => handlerOf<ChatErrorEvent>(onChatError)(overloaded));
    expect(screen.getByText(/Der KI-Anbieter ist gerade überlastet/)).toBeInTheDocument();
    const details = screen.getByTestId("chat-error-details") as HTMLDetailsElement;
    expect(details.open).toBe(false);
    expect(details.textContent).toContain("code: AI_PROVIDER_OVERLOADED");
    expect(details.textContent).toContain("type: overloaded_error");
    expect(details.textContent).toContain("status: 529");
    expect(details.textContent).toContain("message: Overloaded");
    expect(screen.getByRole("button", { name: "Erneut versuchen" })).toBeInTheDocument();
  });

  it("shows 'stream' when there is no HTTP status", async () => {
    await renderWithUserTurn();
    act(() =>
      handlerOf<ChatErrorEvent>(onChatError)({ ...overloaded, httpStatus: undefined }),
    );
    expect(screen.getByTestId("chat-error-details").textContent).toContain("status: stream");
  });

  it("Try again resends once and removes the error card", async () => {
    await renderWithUserTurn();
    act(() => handlerOf<ChatErrorEvent>(onChatError)(overloaded));
    fireEvent.click(screen.getByRole("button", { name: "Erneut versuchen" }));
    await waitFor(() => expect(retryChatTurn).toHaveBeenCalledTimes(1));
    expect(retryChatTurn).toHaveBeenCalledWith("session-1");
    expect(screen.queryByRole("button", { name: "Erneut versuchen" })).toBeNull();
    expect(screen.getAllByText("check the disk")).toHaveLength(1);
  });

  it("offers no Try again for non-retryable codes", async () => {
    await renderWithUserTurn();
    act(() =>
      handlerOf<ChatErrorEvent>(onChatError)({
        sessionId: "session-1",
        message: "x",
        code: "AI_AUTH_FAILED",
      }),
    );
    expect(screen.queryByRole("button", { name: "Erneut versuchen" })).toBeNull();
  });

  it("offers no Try again once a reply followed the user message", async () => {
    await renderWithUserTurn();
    act(() => handlerOf<{ sessionId: string; delta: string }>(onChatTextDelta)({ sessionId: "session-1", delta: "partial" }));
    act(() => handlerOf<ChatErrorEvent>(onChatError)(overloaded));
    expect(screen.queryByRole("button", { name: "Erneut versuchen" })).toBeNull();
  });
});

describe("ChatPanel retry countdown (issue #600)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("counts down and disappears on the first content", async () => {
    vi.mocked(getChatHistory).mockResolvedValue([]);
    render(
      <I18nextProvider i18n={testI18n}>
        <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
      </I18nextProvider>,
    );
    await waitFor(() => expect(onChatAiRetrying).toHaveBeenCalled());
    act(() =>
      handlerOf<ChatAiRetryingEvent>(onChatAiRetrying)({
        sessionId: "session-1",
        attempt: 1,
        maxAttempts: 3,
        delaySecs: 5,
      }),
    );
    const line = screen.getByTestId("chat-ai-retrying");
    expect(line.textContent).toContain("in 5 s");
    expect(line.textContent).toContain("Versuch 2 von 4");
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(screen.getByTestId("chat-ai-retrying").textContent).toContain("in 2 s");
    act(() =>
      handlerOf<{ sessionId: string; delta: string }>(onChatTextDelta)({ sessionId: "session-1", delta: "ok" }),
    );
    expect(screen.queryByTestId("chat-ai-retrying")).toBeNull();
  });
});
