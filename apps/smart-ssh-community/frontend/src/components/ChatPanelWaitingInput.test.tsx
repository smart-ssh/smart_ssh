// Issue #327: hint "probably waiting for input" next to the stop button.
import { act, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  onChatActionOutput,
  onChatActionProposed,
  onChatActionResult,
  onChatActionWaitingInput,
} from "../events";
import type {
  ChatActionProposedEvent,
  ChatActionResultEvent,
  ChatActionWaitingInputEvent,
} from "../types";
import { testI18n } from "../testI18n";
import { ChatPanel } from "./ChatPanel";

if (!Element.prototype.scrollTo) {
  Element.prototype.scrollTo = () => {};
}

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  acceptAndCreateRule: vi.fn(),
  cancelRunningCommand: vi.fn(),
  continueTruncatedResponse: vi.fn(() => Promise.resolve()),
  exportDocument: vi.fn(),
  getChatHistory: vi.fn(() => Promise.resolve([])),
  listAiProviders: vi.fn(() => Promise.resolve([])),
  listPromptHistory: vi.fn(() => Promise.resolve([])),
  respondToAction: vi.fn(() => Promise.resolve()),
  sendChatMessage: vi.fn(() => Promise.resolve()),
  startNewChat: vi.fn(() => Promise.resolve("new-chat-id")),
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
  onChatAiRetrying: vi.fn(() => Promise.resolve(() => {})),
  onChatQueuedMessagesSent: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseCancelled: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseEmpty: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseRefused: vi.fn(() => Promise.resolve(() => {})),
  onChatResponseTruncated: vi.fn(() => Promise.resolve(() => {})),
  onChatContextUsage: () => Promise.resolve(() => {}),
  onChatContextCompacted: () => Promise.resolve(() => {}),
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

const proposed: ChatActionProposedEvent = {
  sessionId: "session-1",
  actionId: "action-1",
  action: { SuggestCommand: { command: "apt install foo" } },
  decision: "AutoExec",
  previousNoteContent: null,
  usesStoredSudoPassword: false,
  previousFileContent: null,
  previousFileSize: null,
  targetName: null,
  riskAssessment: null,
  origin: { kind: "internal" },
};

const waiting = (value: boolean, sessionId = "session-1"): ChatActionWaitingInputEvent => ({
  sessionId,
  actionId: "action-1",
  waiting: value,
});

async function renderRunningCommand() {
  render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
    </I18nextProvider>,
  );
  await waitFor(() => expect(onChatActionOutput).toHaveBeenCalled());
  act(() => handlerOf<ChatActionProposedEvent>(onChatActionProposed)(proposed));
}

describe("ChatPanel waiting-for-input hint (issue #327)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows no hint and no stop button in the first seconds", async () => {
    await renderRunningCommand();
    expect(screen.queryByText(/wartet vermutlich auf eine Eingabe/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /Verbindung zu diesem Kommando trennen/i })).toBeNull();
  });

  it("shows the hint and the stop button immediately, and removes the hint on new output", async () => {
    await renderRunningCommand();
    const emit = handlerOf<ChatActionWaitingInputEvent>(onChatActionWaitingInput);

    act(() => emit(waiting(true)));
    expect(await screen.findByText(/wartet vermutlich auf eine Eingabe/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Verbindung zu diesem Kommando trennen/i })).toBeInTheDocument();

    act(() => emit(waiting(false)));
    expect(screen.queryByText(/wartet vermutlich auf eine Eingabe/i)).toBeNull();
  });

  it("ignores events of another session and clears the hint with the result", async () => {
    await renderRunningCommand();
    const emit = handlerOf<ChatActionWaitingInputEvent>(onChatActionWaitingInput);
    act(() => emit(waiting(true, "other-session")));
    expect(screen.queryByText(/wartet vermutlich auf eine Eingabe/i)).toBeNull();

    act(() => emit(waiting(true)));
    expect(await screen.findByText(/wartet vermutlich auf eine Eingabe/i)).toBeInTheDocument();
    act(() =>
      handlerOf<ChatActionResultEvent>(onChatActionResult)({
        sessionId: "session-1",
        actionId: "action-1",
        result: {
          kind: "command",
          command: "apt install foo",
          stdout: "",
          stderr: "",
          exitCode: null,
          cancelled: true,
          truncated: false,
        },
      }),
    );
    expect(screen.queryByText(/wartet vermutlich auf eine Eingabe/i)).toBeNull();
  });
});
