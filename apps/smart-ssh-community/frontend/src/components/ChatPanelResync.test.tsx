// Issue #1020, Spec 0114 §4: the chat is rebuilt from the backend when the
// app returns from the background.
import { act, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getChatHistory, getChatResync } from "../api";
import { onChatTextDelta } from "../events";
import type { ChatResyncDto } from "../chatResync";
import { testI18n } from "../testI18n";
import { ChatPanel } from "./ChatPanel";

if (!Element.prototype.scrollTo) {
  Element.prototype.scrollTo = () => {};
}

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err ? String((err as { message: unknown }).message) : String(err),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err ? String((err as { code: unknown }).code) : null,
  acceptAndCreateRule: vi.fn(),
  cancelRunningCommand: vi.fn(),
  continueTruncatedResponse: vi.fn(() => Promise.resolve()),
  exportDocument: vi.fn(),
  getChatHistory: vi.fn(() => Promise.resolve([])),
  getChatResync: vi.fn(),
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
  explainConfirmation: vi.fn(() => Promise.resolve({ segments: [], suggestions: [] })),
  takeChatContentIntoNote: vi.fn(),
}));

vi.mock("../events", () => ({
  onActionConfirmationSettled: vi.fn(() => Promise.resolve(() => {})),
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
  onChatAiRequestStarted: vi.fn(() => Promise.resolve(() => {})),
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


function renderChatPanel() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
    </I18nextProvider>,
  );
}

const PENDING = {
  sessionId: "session-1",
  actionId: "action-1",
  action: { SuggestCommand: { command: "uname -a" } },
  decision: { Confirm: { reason: "no rule", code: "FILTER_NO_RULE_MATCHED" } },
  previousNoteContent: null,
  usesStoredSudoPassword: false,
  previousFileContent: null,
  previousFileSize: null,
  targetName: null,
  riskAssessment: null,
  origin: { kind: "internal" },
  autoExecRules: [],
} as unknown as NonNullable<ChatResyncDto["pendingProposal"]>;

const RESULT_ENTRY = {
  type: "commandResult" as const,
  role: "action_result" as const,
  command: "uptime",
  stdout: "up 3 days",
  stderr: "",
  exitCode: 0,
  cancelled: false,
  truncated: false,
};

async function resume(resync: ChatResyncDto) {
  vi.mocked(getChatResync).mockResolvedValue(resync);
  act(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await waitFor(() => expect(getChatResync).toHaveBeenCalledWith("session-1"));
}

describe("ChatPanel re-sync on resume (issue #1020)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getChatHistory).mockResolvedValue([]);
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
  });

  it("shows the assistant text and action result missed in the background", async () => {
    renderChatPanel();
    await resume({
      history: [
        { type: "text", role: "user", text: "how long is it up?" },
        RESULT_ENTRY,
        { type: "text", role: "assistant", text: "Three days." },
      ],
      pendingProposal: null,
    });
    expect(await screen.findByText("Three days.")).toBeInTheDocument();
    expect(screen.getByText("how long is it up?")).toBeInTheDocument();
    expect(screen.getByText(/up 3 days/)).toBeInTheDocument();
  });

  it("shows a still-pending confirmation with active buttons", async () => {
    renderChatPanel();
    await resume({ history: [], pendingProposal: PENDING });
    expect(await screen.findByRole("button", { name: "Ausführen" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Ablehnen" })).toBeEnabled();
  });

  it("shows a confirmation settled meanwhile as settled, without buttons", async () => {
    renderChatPanel();
    await resume({ history: [], pendingProposal: PENDING });
    await screen.findByRole("button", { name: "Ausführen" });
    await resume({ history: [RESULT_ENTRY], pendingProposal: null });
    await waitFor(() => expect(screen.queryByRole("button", { name: "Ausführen" })).toBeNull());
    expect(screen.getByText(/up 3 days/)).toBeInTheDocument();
  });

  it("renders an action result text as a result, not as an assistant bubble", async () => {
    renderChatPanel();
    await resume({
      history: [{ type: "text", role: "action_result", text: "Note updated." }],
      pendingProposal: null,
    });
    const result = await screen.findByTestId("history-action-result");
    expect(result).toHaveTextContent("Note updated.");
  });

  it("keeps the partial answer of a streaming turn once and continues it", async () => {
    renderChatPanel();
    await waitFor(() => expect(onChatTextDelta).toHaveBeenCalled());
    const onDelta = vi.mocked(onChatTextDelta).mock.calls.at(-1)![0];
    act(() => onDelta({ sessionId: "session-1", delta: "Hello" }));
    await screen.findByText("Hello");
    await resume({
      history: [{ type: "text", role: "user", text: "hi" }],
      pendingProposal: null,
    });
    await screen.findByText("hi");
    act(() => onDelta({ sessionId: "session-1", delta: " world" }));
    expect(await screen.findByText("Hello world")).toBeInTheDocument();
    expect(screen.getAllByText(/Hello/)).toHaveLength(1);
  });

  it("does not duplicate an answer the history already holds", async () => {
    renderChatPanel();
    await waitFor(() => expect(onChatTextDelta).toHaveBeenCalled());
    const onDelta = vi.mocked(onChatTextDelta).mock.calls.at(-1)![0];
    act(() => onDelta({ sessionId: "session-1", delta: "Done." }));
    await screen.findByText("Done.");
    await resume({
      history: [
        { type: "text", role: "user", text: "do it" },
        { type: "text", role: "assistant", text: "Done." },
      ],
      pendingProposal: null,
    });
    await screen.findByText("do it");
    expect(screen.getAllByText("Done.")).toHaveLength(1);
  });
});
