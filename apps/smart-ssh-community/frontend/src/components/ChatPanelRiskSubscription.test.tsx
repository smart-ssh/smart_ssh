// Issue #829: the chat event listeners stay registered when the risk second
// opinion setting changes while an answer streams.
import { act, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getChatHistory } from "../api";
import { onChatActionProposed, onChatTextDelta } from "../events";
import { loadRiskClassifierSettings } from "../riskSettings";
import type { ChatActionProposedEvent } from "../types";
import { testI18n } from "../testI18n";
import { renderChatPanel } from "../test/renderChatPanel";

import "../test/scrollTo";

vi.mock("../api", async () => {
  const { anthropicProvider } = await import("../test/fixtures");
  const { createApiMock } = await import("../test/apiMock");
  return createApiMock({
    listAiProviders: vi.fn(() => Promise.resolve([anthropicProvider()])),
    explainConfirmation: vi.fn(() => new Promise<never>(() => {})),
  });
});

vi.mock("../events", async () =>
  (await import("../test/eventsMock")).createEventsMock(),
);

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(),
}));

type Delta = { sessionId: string; delta: string };

/** Registers like the real `listen()`: resolves asynchronously, and tracks how
 * many listeners are active and whether the count ever dropped to zero. */
function trackAsyncListener<T>(listener: unknown) {
  const state = {
    active: 0,
    handler: null as null | ((e: T) => void),
    droppedToZero: false,
    started: 0,
  };
  vi.mocked(
    listener as (h: (e: T) => void) => Promise<() => void>,
  ).mockImplementation((h) => {
    state.started += 1;
    return new Promise((resolve) => {
      setTimeout(() => {
        state.active += 1;
        state.handler = h;
        resolve(() => {
          state.active -= 1;
          if (state.active === 0) state.droppedToZero = true;
        });
      }, 0);
    });
  });
  return state;
}

function deferredSettings() {
  let resolve: (enabled: boolean) => void = () => {};
  vi.mocked(loadRiskClassifierSettings).mockImplementation(
    () =>
      new Promise(
        (r) => (resolve = (enabled) => r({ enabled, providerId: null })),
      ),
  );
  return (enabled: boolean) => resolve(enabled);
}

function proposed(): ChatActionProposedEvent {
  return {
    sessionId: "session-1",
    actionId: "a1",
    action: { SuggestCommand: { command: "ls" } },
    decision: {
      Confirm: { reason: "no rule", code: "FILTER_NO_RULE_MATCHED" },
    },
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    previousFileContent: null,
    previousFileSize: null,
    targetName: null,
    riskAssessment: {
      serverRisk: "none",
      serverRiskReason: null,
      dataRisk: "yellow",
      dataRiskReason: "x",
      aiReviewed: false,
    },
    origin: "ai",
    autoExecRules: [],
  } as unknown as ChatActionProposedEvent;
}

function renderPanel() {
  renderChatPanel();
}

describe("ChatPanel event subscription vs. risk second opinion setting (issue #829)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getChatHistory).mockResolvedValue([]);
  });

  it("keeps the listener and loses no delta when the setting changes mid-stream", async () => {
    const delta = trackAsyncListener<Delta>(onChatTextDelta);
    const settle = deferredSettings();
    renderPanel();
    await waitFor(() => expect(delta.active).toBe(1));
    act(() => delta.handler?.({ sessionId: "session-1", delta: "Hel" }));
    await act(async () => settle(true));
    // Give a (wrongly triggered) re-subscription time to run.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    act(() => delta.handler?.({ sessionId: "session-1", delta: "lo " }));
    act(() => delta.handler?.({ sessionId: "session-1", delta: "world" }));
    expect(await screen.findByText("Hello world")).toBeInTheDocument();
    expect(delta.started).toBe(1);
    expect(delta.droppedToZero).toBe(false);
  });

  it("shows the pending second opinion after the setting turned on", async () => {
    const proposal =
      trackAsyncListener<ChatActionProposedEvent>(onChatActionProposed);
    const settle = deferredSettings();
    renderPanel();
    await waitFor(() => expect(proposal.active).toBe(1));
    await act(async () => settle(true));
    act(() => proposal.handler?.(proposed()));
    const title = testI18n.t("actionCard.riskSecondOpinionPending");
    await waitFor(() =>
      expect(document.querySelector(`[title="${title}"]`)).not.toBeNull(),
    );
    expect(proposal.started).toBe(1);
  });

  it("shows no pending second opinion when the setting is off", async () => {
    const proposal =
      trackAsyncListener<ChatActionProposedEvent>(onChatActionProposed);
    const settle = deferredSettings();
    renderPanel();
    await waitFor(() => expect(proposal.active).toBe(1));
    await act(async () => settle(false));
    act(() => proposal.handler?.(proposed()));
    await screen.findByText("ls", { selector: "code" });
    const title = testI18n.t("actionCard.riskSecondOpinionPending");
    expect(document.querySelector(`[title="${title}"]`)).toBeNull();
  });
});
