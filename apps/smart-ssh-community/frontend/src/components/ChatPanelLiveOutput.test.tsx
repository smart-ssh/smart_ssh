// Issue #325, Spec 0106: live output of a running command in the chat —
// appears inside the command's block while it runs, stdout and stderr
// apart, and is replaced by the final result without duplicated output.
import { act, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { onChatActionOutput, onChatActionProposed, onChatActionResult } from "../events";
import type {
  ChatActionOutputEvent,
  ChatActionProposedEvent,
  ChatActionResultEvent,
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

function handlerOf<T>(listener: unknown): (event: T) => void {
  const calls = vi.mocked(listener as (h: (event: T) => void) => unknown).mock.calls;
  return calls[calls.length - 1][0];
}

function proposed(sessionId = "session-1"): ChatActionProposedEvent {
  return {
    sessionId,
    actionId: "action-1",
    action: { SuggestCommand: { command: "for i in 1 2 3; do echo $i; sleep 1; done" } },
    decision: "AutoExec",
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    previousFileContent: null,
    previousFileSize: null,
    targetName: null,
    riskAssessment: null,
    origin: { kind: "internal" },
  };
}

function output(
  stdout: string,
  stderr = "",
  truncated = false,
  sessionId = "session-1",
): ChatActionOutputEvent {
  return { sessionId, actionId: "action-1", stdout, stderr, truncated };
}

async function renderRunningCommand() {
  render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel sessionId="session-1" serverId="server-1" onActionSettled={vi.fn()} />
    </I18nextProvider>,
  );
  await waitFor(() => expect(onChatActionOutput).toHaveBeenCalled());
  act(() => handlerOf<ChatActionProposedEvent>(onChatActionProposed)(proposed()));
}

describe("ChatPanel live command output (issue #325)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows lines one after another while the command runs", async () => {
    await renderRunningCommand();
    const emit = handlerOf<ChatActionOutputEvent>(onChatActionOutput);

    act(() => emit(output("1\n")));
    const live = await screen.findByTestId("live-command-output");
    expect(live).toHaveTextContent("1");
    act(() => emit(output("2\n")));
    expect(live.querySelector("pre")?.textContent).toBe("1\n2\n");
  });

  it("shows stderr apart from stdout, in the stderr colour", async () => {
    await renderRunningCommand();
    act(() => handlerOf<ChatActionOutputEvent>(onChatActionOutput)(output("ok\n", "warn\n")));

    const live = await screen.findByTestId("live-command-output");
    const stderr = within(live).getByText("warn");
    expect(stderr).toHaveClass("text-red-300");
    expect(within(live).getByText("ok")).toHaveClass("text-slate-300");
  });

  it("shows the size-limit notice in the live view", async () => {
    await renderRunningCommand();
    act(() => handlerOf<ChatActionOutputEvent>(onChatActionOutput)(output("a\n", "", true)));

    expect(
      await screen.findByText(testI18n.t("actionCard.liveOutputTruncatedNotice")),
    ).toBeInTheDocument();
  });

  it("ignores live output of another session", async () => {
    await renderRunningCommand();
    act(() =>
      handlerOf<ChatActionOutputEvent>(onChatActionOutput)(
        output("other\n", "", false, "session-2"),
      ),
    );

    expect(screen.queryByTestId("live-command-output")).not.toBeInTheDocument();
  });

  it("replaces the live view with the final result, without duplicated output", async () => {
    await renderRunningCommand();
    act(() => handlerOf<ChatActionOutputEvent>(onChatActionOutput)(output("line one\n")));
    expect(await screen.findByTestId("live-command-output")).toBeInTheDocument();

    const result: ChatActionResultEvent = {
      sessionId: "session-1",
      actionId: "action-1",
      result: {
        kind: "command",
        command: "x",
        stdout: "line one\nline two\n",
        stderr: "",
        exitCode: 0,
        cancelled: false,
        truncated: false,
      },
    };
    act(() => handlerOf<ChatActionResultEvent>(onChatActionResult)(result));

    await waitFor(() =>
      expect(screen.queryByTestId("live-command-output")).not.toBeInTheDocument(),
    );
    expect(screen.getAllByText(/line one/)).toHaveLength(1);
  });
});
