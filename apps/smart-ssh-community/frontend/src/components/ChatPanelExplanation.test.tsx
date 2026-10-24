// Issue #596, Spec 0011: the confirmation card lists which segments existing
// rules cover, shows a verdict per suggestion, and notices when an accepted
// rule does not make the command run automatically.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { acceptAndCreateRule, explainConfirmation } from "../api";
import { onChatActionOutput, onChatActionProposed } from "../events";
import type { ChatActionProposedEvent, ConfirmationExplanationDto } from "../types";
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
  onChatContextUsage: vi.fn(() => Promise.resolve(() => {})),
  onChatContextCompacted: vi.fn(() => Promise.resolve(() => {})),
  onChatAiRetrying: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationLimitReached: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationStarted: vi.fn(() => Promise.resolve(() => {})),
  onChatDocumentGenerated: vi.fn(() => Promise.resolve(() => {})),
  onChatQuestionAsked: vi.fn(() => Promise.resolve(() => {})),
  onChatError: vi.fn(() => Promise.resolve(() => {})),
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

const COMMAND = "grep a f | grep -v b | wc -l";

const EXPLANATION: ConfirmationExplanationDto = {
  segments: [
    { index: 0, text: "grep a f", state: "covered", rule: { patternType: "glob", patternValue: "grep *" }, reasons: [] },
    { index: 1, text: "grep -v b", state: "covered", rule: { patternType: "glob", patternValue: "grep *" }, reasons: [] },
    { index: 2, text: "wc -l", state: "needsRule", rule: null, reasons: [] },
  ],
  suggestions: [
    {
      kind: "tool",
      names: ["wc"],
      patterns: [{ patternType: "glob", patternValue: "wc *" }],
      verdict: { autoExec: false, reasons: [{ code: "FILTER_OUTPUT_REDIRECTION", segmentIndex: 2 }] },
    },
  ],
};

async function renderCard(explanation: ConfirmationExplanationDto) {
  vi.mocked(explainConfirmation).mockResolvedValue(explanation);
  await testI18n.changeLanguage("en");
  render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
    </I18nextProvider>,
  );
  await waitFor(() => expect(onChatActionOutput).toHaveBeenCalled());
  const event: ChatActionProposedEvent = {
    sessionId: "session-1",
    actionId: "action-1",
    action: { SuggestCommand: { command: COMMAND } },
    decision: { Confirm: { reason: "no rule", code: "FILTER_NO_RULE_MATCHED" } },
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    previousFileContent: null,
    previousFileSize: null,
    targetName: null,
    riskAssessment: null,
    origin: { kind: "internal" },
  };
  act(() => handlerOf<ChatActionProposedEvent>(onChatActionProposed)(event));
}

describe("confirmation card explanation (issue #596)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(acceptAndCreateRule).mockResolvedValue([]);
  });
  afterEach(async () => {
    await testI18n.changeLanguage("de");
  });

  it("lists each segment with its coverage state", async () => {
    await renderCard(EXPLANATION);
    expect(await screen.findByText("grep -v b")).toBeInTheDocument();
    expect(screen.getAllByText(/✓ covered by rule grep \*/)).toHaveLength(2);
    expect(screen.getByText("needs a rule")).toBeInTheDocument();
  });

  it("shows nothing extra for a single segment without a reason", async () => {
    await renderCard({ segments: [], suggestions: [] });
    await waitFor(() => expect(explainConfirmation).toHaveBeenCalled());
    expect(screen.queryByText("Parts of this command")).toBeNull();
  });

  it("posts a notice with the reason after accepting a will-still-ask suggestion", async () => {
    await renderCard(EXPLANATION);
    fireEvent.click(await screen.findByLabelText("Show rule suggestions"));
    expect(await screen.findByText(/will still ask/)).toBeInTheDocument();
    fireEvent.click(screen.getByText(/All `wc` calls/));
    expect(
      await screen.findByText(/Rule created\. This command will still ask next time: .*redirection/i),
    ).toBeInTheDocument();
  });

  it("posts no notice after accepting a suggestion that runs automatically", async () => {
    await renderCard({
      ...EXPLANATION,
      suggestions: [{ ...EXPLANATION.suggestions[0], verdict: { autoExec: true, reasons: [] } }],
    });
    fireEvent.click(await screen.findByLabelText("Show rule suggestions"));
    expect(await screen.findByText("runs automatically next time")).toBeInTheDocument();
    fireEvent.click(screen.getByText(/All `wc` calls/));
    await waitFor(() => expect(acceptAndCreateRule).toHaveBeenCalled());
    expect(screen.queryByText(/Rule created\./)).toBeNull();
  });
});
