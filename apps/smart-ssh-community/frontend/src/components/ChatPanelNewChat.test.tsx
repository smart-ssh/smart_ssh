// Issue #271, Spec 0034 §11: „Neuer Chat" im Chat-Panel einer verbundenen
// Sitzung — gesperrt während eines Turns bzw. bei offener Bestätigung, leert
// bei Erfolg die Chat-Ansicht.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { sendChatMessage, startNewChat } from "../api";
import { onChatActionProposed } from "../events";
import type { ChatActionProposedEvent } from "../types";
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
  onChatActionResult: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationLimitReached: vi.fn(() => Promise.resolve(() => {})),
  onChatAutoContinuationStarted: vi.fn(() => Promise.resolve(() => {})),
  onChatDocumentGenerated: vi.fn(() => Promise.resolve(() => {})),
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
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

const PLACEHOLDER = "Frage stellen oder Kommando beschreiben …";

function renderChatPanel(readOnlyHint?: string) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel
        sessionId="session-1"
        serverId="server-1"
        onActionSettled={vi.fn()}
        readOnlyHint={readOnlyHint}
      />
    </I18nextProvider>,
  );
}

function confirmEvent(): ChatActionProposedEvent {
  return {
    sessionId: "session-1",
    actionId: "action-1",
    action: { SuggestCommand: { command: "printenv" } },
    decision: { Confirm: { reason: "keine Regel gefunden", code: "FILTER_NO_RULE_MATCHED" } },
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    previousFileContent: null,
    previousFileSize: null,
    targetName: null,
    riskAssessment: null,
    origin: { kind: "internal" },
  };
}

describe("ChatPanel „Neuer Chat“ (issue #271)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("empties the chat view and calls the backend when clicked", async () => {
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "hallo Welt" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(sendChatMessage).toHaveBeenCalled());
    expect(await screen.findByText("hallo Welt")).toBeInTheDocument();

    const button = await screen.findByRole("button", { name: "Neuer Chat" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);

    await waitFor(() => expect(startNewChat).toHaveBeenCalledWith("session-1"));
    await waitFor(() => expect(screen.queryByText("hallo Welt")).not.toBeInTheDocument());
  });

  it("is disabled while an AI turn is running", async () => {
    vi.mocked(sendChatMessage).mockImplementation(() => new Promise<void>(() => {}));
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "lange Frage" } });
    fireEvent.keyDown(field, { key: "Enter" });

    const button = await screen.findByRole("button", { name: "Neuer Chat" });
    await waitFor(() => expect(button).toBeDisabled());
    fireEvent.click(button);
    expect(startNewChat).not.toHaveBeenCalled();
  });

  it("is disabled while a confirmation is open", async () => {
    let proposedHandler: ((event: ChatActionProposedEvent) => void) | null = null;
    vi.mocked(onChatActionProposed).mockImplementation((h) => {
      proposedHandler = h;
      return Promise.resolve(() => {});
    });
    renderChatPanel();
    await waitFor(() => expect(proposedHandler).not.toBeNull());
    const button = await screen.findByRole("button", { name: "Neuer Chat" });
    await waitFor(() => expect(button).toBeEnabled());

    act(() => {
      proposedHandler!(confirmEvent());
    });

    expect(await screen.findByRole("button", { name: "Ausführen" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Neuer Chat" })).toBeDisabled();
  });

  it("is not shown in a read-only (MCP) panel", async () => {
    renderChatPanel("nur lesen");
    await screen.findByText("nur lesen");
    expect(screen.queryByRole("button", { name: "Neuer Chat" })).not.toBeInTheDocument();
  });
});
