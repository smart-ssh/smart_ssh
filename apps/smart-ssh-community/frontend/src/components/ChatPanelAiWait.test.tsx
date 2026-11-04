// Issue #694: AI wait line and named timeout errors.
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getChatHistory, sendChatMessage } from "../api/api";
import {
  onChatAiRequestStarted,
  onChatError,
  onChatTextDelta,
} from "../api/events";
import { chatError } from "../test/fixtures";
import type { ChatAiRequestStartedEvent, ChatErrorEvent } from "../api/types";
import { renderChatPanel } from "../test/renderChatPanel";

import "../test/scrollTo";

vi.mock("../api/api", async () => {
  const { anthropicProvider } = await import("../test/fixtures");
  const { createApiMock } = await import("../test/apiMock");
  return createApiMock({
    listAiProviders: vi.fn(() => Promise.resolve([anthropicProvider()])),
  });
});

vi.mock("../api/events", async () =>
  (await import("../test/eventsMock")).createEventsMock(),
);

vi.mock("../settings/riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() =>
    Promise.resolve({ enabled: false, providerId: null }),
  ),
}));

function handlerOf<T>(listener: unknown): (event: T) => void {
  const calls = vi.mocked(listener as (h: (event: T) => void) => unknown).mock
    .calls;
  return calls[calls.length - 1][0];
}

async function renderSending() {
  vi.mocked(getChatHistory).mockResolvedValue([
    { type: "text", role: "user", text: "check the disk" },
  ] as never);
  vi.mocked(sendChatMessage).mockImplementation(() => new Promise(() => {}));
  renderChatPanel();
  await screen.findByText("check the disk");
  await waitFor(() => expect(onChatAiRequestStarted).toHaveBeenCalled());
  const field = await screen.findByPlaceholderText(
    "Frage stellen oder Kommando beschreiben …",
  );
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
    renderChatPanel();
    await screen.findByText("check the disk");
    await waitFor(() => expect(onChatAiRequestStarted).toHaveBeenCalled());
    const field = await screen.findByPlaceholderText(
      "Frage stellen oder Kommando beschreiben …",
    );
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
    // wait-ok: the waiting indicator is driven only by the request-started event, which this test never emits again; nothing renders later
    await waitFor(() => expect(sendChatMessage).toHaveBeenCalledTimes(2));
    expect(screen.queryByTestId("chat-ai-waiting")).toBeNull();
  });

  it("names the provider on a timeout and offers Try again", async () => {
    vi.mocked(getChatHistory).mockResolvedValue([
      { type: "text", role: "user", text: "check the disk" },
    ] as never);
    renderChatPanel();
    await screen.findByText("check the disk");
    await waitFor(() => expect(onChatError).toHaveBeenCalled());
    act(() =>
      handlerOf<ChatErrorEvent>(onChatError)(chatError({
        sessionId: "session-1",
        message: "x",
        code: "AI_TIMEOUT",
        providerName: "Claude",
        timeoutSecs: 120,
      })),
    );
    expect(screen.getByText(/Claude/).textContent).toContain("120 s");
    expect(
      screen.getByRole("button", { name: "Erneut versuchen" }),
    ).toBeInTheDocument();
  });

  it("names the provider on the total deadline and offers Try again", async () => {
    vi.mocked(getChatHistory).mockResolvedValue([
      { type: "text", role: "user", text: "check the disk" },
    ] as never);
    renderChatPanel();
    await screen.findByText("check the disk");
    await waitFor(() => expect(onChatError).toHaveBeenCalled());
    act(() =>
      handlerOf<ChatErrorEvent>(onChatError)(chatError({
        sessionId: "session-1",
        message: "x",
        code: "AI_TOTAL_DEADLINE",
        providerName: "Claude",
        timeoutSecs: 600,
      })),
    );
    expect(screen.getByText(/Claude/).textContent).toContain("10 Minuten");
    expect(
      screen.getByRole("button", { name: "Erneut versuchen" }),
    ).toBeInTheDocument();
  });
});
