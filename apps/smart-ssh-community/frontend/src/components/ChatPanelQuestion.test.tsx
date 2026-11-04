// Issue #271, Spec 0034 §11: „Neuer Chat" im Chat-Panel einer verbundenen
// Sitzung — gesperrt während eines Turns bzw. bei offener Bestätigung, leert
// bei Erfolg die Chat-Ansicht.
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getChatHistory, sendChatMessage } from "../api/api";
import { onChatQuestionAsked } from "../api/events";
import type { ChatQuestionAskedEvent } from "../api/types";
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

const PLACEHOLDER = "Frage stellen oder Kommando beschreiben …";

const OPTIONS = [
  { label: "Alpha", description: "first <b>bold</b>" },
  { label: "Beta", description: null },
];

function question(
  multiSelect = false,
  text = "Which one?",
): ChatQuestionAskedEvent {
  return {
    sessionId: "session-1",
    questionId: "q1",
    question: text,
    options: OPTIONS,
    multiSelect,
  };
}

async function ask(event: ChatQuestionAskedEvent) {
  await waitFor(() => expect(onChatQuestionAsked).toHaveBeenCalled());
  const handler = vi.mocked(onChatQuestionAsked).mock.calls.at(-1)![0];
  act(() => handler(event));
}

describe("ChatPanel question card (issue #547)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getChatHistory).mockResolvedValue([]);
  });

  it("renders the card with question, labels and descriptions as plain text", async () => {
    renderChatPanel();
    await ask(question(false, "Pick <i>one</i>"));
    expect(
      await screen.findByRole("group", { name: /Pick <i>one<\/i>/ }),
    ).toBeInTheDocument();
    expect(screen.getByText("Pick <i>one</i>")).toBeInTheDocument();
    expect(screen.getByText("first <b>bold</b>")).toBeInTheDocument();
    expect(document.querySelector("b, i")).toBeNull();
  });

  it("sends the label on click, shows a user bubble and locks the card", async () => {
    renderChatPanel();
    await ask(question());
    fireEvent.click(await screen.findByRole("button", { name: /Beta/ }));
    await waitFor(() => {
      const group = screen.getByRole("group");
      expect(
        within(group).getByRole("button", { name: /Alpha/ }),
      ).toBeDisabled();
      expect(
        within(group).getByRole("button", { name: /Beta/ }),
      ).toHaveTextContent("✓");
      expect(screen.getAllByText("Beta")).toHaveLength(2);
    });
    expect(sendChatMessage).toHaveBeenCalledWith("session-1", "Beta");
  });

  it("sends ticked labels one per line in option order for multi select", async () => {
    renderChatPanel();
    await ask(question(true));
    const send = within(await screen.findByRole("group")).getByRole("button", {
      name: "Senden",
    });
    expect(send).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: /Beta/ }));
    fireEvent.click(screen.getByRole("button", { name: /Alpha/ }));
    fireEvent.click(send);
    await waitFor(() =>
      expect(sendChatMessage).toHaveBeenCalledWith("session-1", "Alpha\nBeta"),
    );
  });

  it("keys 1-4 act while the card has focus, not in the composer", async () => {
    renderChatPanel();
    await ask(question());
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.keyDown(field, { key: "1" });
    expect(sendChatMessage).not.toHaveBeenCalled();
    fireEvent.keyDown(await screen.findByRole("button", { name: /Alpha/ }), {
      key: "1",
    });
    await waitFor(() =>
      expect(sendChatMessage).toHaveBeenCalledWith("session-1", "Alpha"),
    );
  });

  it("locks with the own-words note when the user types instead", async () => {
    renderChatPanel();
    await ask(question());
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "something else" } });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(
      await screen.findByText("In eigenen Worten beantwortet"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("group")).getByRole("button", { name: /Alpha/ }),
    ).toBeDisabled();
  });

  it("keeps only the latest unanswered card active", async () => {
    renderChatPanel();
    await ask(question(false, "First?"));
    await ask({ ...question(false, "Second?"), questionId: "q2" });
    const groups = await screen.findAllByRole("group");
    expect(
      within(groups[0]).getByRole("button", { name: /Alpha/ }),
    ).toBeDisabled();
    expect(
      within(groups[1]).getByRole("button", { name: /Alpha/ }),
    ).toBeEnabled();
  });

  it("queues the answer while a turn runs", async () => {
    vi.mocked(sendChatMessage).mockImplementation(() => new Promise(() => {}));
    renderChatPanel();
    const field = await screen.findByPlaceholderText(PLACEHOLDER);
    fireEvent.change(field, { target: { value: "go" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(sendChatMessage).toHaveBeenCalledTimes(1));
    await ask(question());
    fireEvent.click(await screen.findByRole("button", { name: /Alpha/ }));
    expect(
      (await screen.findAllByText(/Wird mit der nächsten Anfrage gesendet/))
        .length,
    ).toBeGreaterThan(0);
    expect(sendChatMessage).toHaveBeenCalledWith("session-1", "Alpha");
  });

  it("derives lock state from the history on reopen", async () => {
    vi.mocked(getChatHistory).mockResolvedValue([
      {
        type: "question",
        role: "assistant",
        questionId: "q1",
        question: "Old?",
        options: OPTIONS,
        multiSelect: false,
      },
      { type: "text", role: "user", text: "Beta" },
      {
        type: "question",
        role: "assistant",
        questionId: "q2",
        question: "New?",
        options: OPTIONS,
        multiSelect: false,
      },
    ]);
    renderChatPanel();
    const groups = await screen.findAllByRole("group");
    expect(
      within(groups[0]).getByRole("button", { name: /Beta/ }),
    ).toHaveTextContent("✓");
    expect(
      within(groups[0]).getByRole("button", { name: /Alpha/ }),
    ).toBeDisabled();
    expect(
      within(groups[1]).getByRole("button", { name: /Alpha/ }),
    ).toBeEnabled();
  });
});
