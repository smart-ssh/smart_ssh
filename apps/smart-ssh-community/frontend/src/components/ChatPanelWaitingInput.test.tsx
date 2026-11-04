// Issue #327: hint "probably waiting for input" next to the stop button.
import { act, screen, waitFor } from "@testing-library/react";
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
import { renderChatPanel } from "../test/renderChatPanel";

import "../test/scrollTo";

vi.mock("../api", async () =>
  (await import("../test/apiMock")).createApiMock(),
);

vi.mock("../events", async () =>
  (await import("../test/eventsMock")).createEventsMock(),
);

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() =>
    Promise.resolve({ enabled: false, providerId: null }),
  ),
}));

function handlerOf<T>(listener: unknown): (event: T) => void {
  const calls = vi.mocked(listener as (h: (event: T) => void) => unknown).mock
    .calls;
  return calls[calls.length - 1][0];
}

const proposed: ChatActionProposedEvent = {
  sessionId: "session-1",
  actionId: "action-1",
  action: { SuggestCommand: { command: "apt install foo" } },
  decision: "AutoExec",
  previousNoteContent: null,
  usesStoredSudoPassword: false,
  autoExecRules: [],
  previousFileContent: null,
  previousFileSize: null,
  targetName: null,
  riskAssessment: null,
  origin: { kind: "internal" },
};

const waiting = (
  value: boolean,
  sessionId = "session-1",
): ChatActionWaitingInputEvent => ({
  sessionId,
  actionId: "action-1",
  waiting: value,
});

async function renderRunningCommand() {
  renderChatPanel();
  await waitFor(() => expect(onChatActionOutput).toHaveBeenCalled());
  act(() => handlerOf<ChatActionProposedEvent>(onChatActionProposed)(proposed));
}

describe("ChatPanel waiting-for-input hint (issue #327)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows no hint and no stop button in the first seconds", async () => {
    await renderRunningCommand();
    expect(
      screen.queryByText(/wartet vermutlich auf eine Eingabe/i),
    ).toBeNull();
    expect(
      screen.queryByRole("button", {
        name: /Verbindung zu diesem Kommando trennen/i,
      }),
    ).toBeNull();
  });

  it("shows the hint and the stop button immediately, and removes the hint on new output", async () => {
    await renderRunningCommand();
    const emit = handlerOf<ChatActionWaitingInputEvent>(
      onChatActionWaitingInput,
    );

    act(() => emit(waiting(true)));
    expect(
      await screen.findByText(/wartet vermutlich auf eine Eingabe/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: /Verbindung zu diesem Kommando trennen/i,
      }),
    ).toBeInTheDocument();

    act(() => emit(waiting(false)));
    expect(
      screen.queryByText(/wartet vermutlich auf eine Eingabe/i),
    ).toBeNull();
  });

  it("ignores events of another session and clears the hint with the result", async () => {
    await renderRunningCommand();
    const emit = handlerOf<ChatActionWaitingInputEvent>(
      onChatActionWaitingInput,
    );
    act(() => emit(waiting(true, "other-session")));
    expect(
      screen.queryByText(/wartet vermutlich auf eine Eingabe/i),
    ).toBeNull();

    act(() => emit(waiting(true)));
    expect(
      await screen.findByText(/wartet vermutlich auf eine Eingabe/i),
    ).toBeInTheDocument();
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
    expect(
      screen.queryByText(/wartet vermutlich auf eine Eingabe/i),
    ).toBeNull();
  });
});
