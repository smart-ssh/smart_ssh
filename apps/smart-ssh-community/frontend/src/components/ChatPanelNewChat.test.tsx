// Issue #271, Spec 0034 §11: „Neuer Chat" im Chat-Panel einer verbundenen
// Sitzung — gesperrt während eines Turns bzw. bei offener Bestätigung, leert
// bei Erfolg die Chat-Ansicht.
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { clickDisabled } from "../test/clickDisabled";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { sendChatMessage, startNewChat } from "../api";
import { onChatActionProposed, onChatContextUsage } from "../events";
import type { ChatActionProposedEvent } from "../types";
import { renderChatPanel } from "../test/renderChatPanel";

import "../test/scrollTo";

vi.mock("../api", async () => {
  const { anthropicProvider } = await import("../test/fixtures");
  const { createApiMock } = await import("../test/apiMock");
  return createApiMock({
    listAiProviders: vi.fn(() => Promise.resolve([anthropicProvider()])),
  });
});

vi.mock("../events", async () =>
  (await import("../test/eventsMock")).createEventsMock(),
);

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() =>
    Promise.resolve({ enabled: false, providerId: null }),
  ),
}));

const PLACEHOLDER = "Frage stellen oder Kommando beschreiben …";

function confirmEvent(): ChatActionProposedEvent {
  return {
    sessionId: "session-1",
    actionId: "action-1",
    action: { SuggestCommand: { command: "printenv" } },
    decision: {
      Confirm: {
        reason: "keine Regel gefunden",
        code: "FILTER_NO_RULE_MATCHED",
      },
    },
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
    expect(await screen.findByText("hallo Welt")).toBeInTheDocument();
    expect(sendChatMessage).toHaveBeenCalled();

    const button = await screen.findByRole("button", { name: "Neuer Chat" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);

    await waitFor(() =>
      expect(screen.queryByText("hallo Welt")).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(startNewChat).toHaveBeenCalledWith("session-1"));
  });

  it("resets the context usage indicator to 0 after a new chat (issue #648)", async () => {
    renderChatPanel();
    await screen.findByPlaceholderText(PLACEHOLDER);
    await waitFor(() => expect(onChatContextUsage).toHaveBeenCalled());
    const handler = vi.mocked(onChatContextUsage).mock.calls[0][0];
    act(() =>
      handler({
        sessionId: "session-1",
        inputTokens: 90_000,
        windowTokens: 200_000,
      }),
    );
    const indicator = await screen.findByTestId("context-usage");
    expect(indicator.textContent).toContain("90");

    const button = await screen.findByRole("button", { name: "Neuer Chat" });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);

    await waitFor(() =>
      expect(screen.getByTestId("context-usage").textContent).not.toContain(
        "90",
      ),
    );
    expect(screen.getByTestId("context-usage").textContent).toContain("200");

    act(() =>
      handler({
        sessionId: "session-1",
        inputTokens: 5_000,
        windowTokens: 200_000,
      }),
    );
    expect(screen.getByTestId("context-usage").textContent).toContain("5");
  });

  it("keeps the text in the input when the backend rejects a message during the swap", async () => {
    vi.mocked(sendChatMessage).mockRejectedValueOnce({
      message: "Gerade wird ein neuer Chat begonnen",
      code: "CHAT_SWITCHING",
    });
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "verlorene Frage" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(field).toHaveValue("verlorene Frage"));
    // Not shown as sent in the history; only the error notice remains.
    expect(screen.getAllByText(/verlorene Frage/)).toHaveLength(1);
    expect(
      screen.getByText(/Gerade wird ein neuer Chat begonnen/),
    ).toBeInTheDocument();
  });

  it("is disabled while an AI turn is running", async () => {
    vi.mocked(sendChatMessage).mockImplementation(
      () => new Promise<void>(() => {}),
    );
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "lange Frage" } });
    fireEvent.keyDown(field, { key: "Enter" });

    const button = await screen.findByRole("button", { name: "Neuer Chat" });
    await waitFor(() => expect(button).toBeDisabled());
    clickDisabled(button);
    expect(startNewChat).not.toHaveBeenCalled();
  });

  it("is disabled while a confirmation is open", async () => {
    let proposedHandler: ((event: ChatActionProposedEvent) => void) | null =
      null;
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

    expect(
      await screen.findByRole("button", { name: "Ausführen" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Neuer Chat" })).toBeDisabled();
  });

  it("is not shown in a read-only (MCP) panel", async () => {
    renderChatPanel({ readOnlyHint: "nur lesen" });
    await screen.findByText("nur lesen");
    expect(
      screen.queryByRole("button", { name: "Neuer Chat" }),
    ).not.toBeInTheDocument();
  });
});
