// Spec 0057, §4.2 (Issue #94): eine vom Provider abgeschnittene Kürzung
// wird im Notiz-Vorschlag als unvollständig markiert — die Warnung kommt
// von der App selbst, über dem Diff, nicht aus dem KI-Text.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { respondToAction } from "../api";
import { onNoteUpdateExpired, onNoteUpdateSuggested } from "../events";
import { testI18n } from "../testI18n";
import type { NoteUpdateSuggestedEvent } from "../types";
import { NoteSuggestionToast } from "./NoteSuggestionToast";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: (err: unknown) =>
    (err as { code?: string } | null)?.code ?? null,
  respondToAction: vi.fn(() => Promise.resolve()),
}));

vi.mock("../events", () => ({
  onNoteUpdateSuggested: vi.fn(() => Promise.resolve(() => {})),
  onNoteUpdateExpired: vi.fn(() => Promise.resolve(() => {})),
}));

const WARNING_DE =
  "Die Zusammenfassung wurde abgeschnitten und ist unvollständig. Prüfe sie sorgfältig oder lehne sie ab.";

async function renderToastAndCaptureEmit() {
  let handler: ((event: NoteUpdateSuggestedEvent) => void) | null = null;
  let expiredHandler: ((event: { actionId: string }) => void) | null = null;
  vi.mocked(onNoteUpdateExpired).mockImplementation((h) => {
    expiredHandler = h;
    return Promise.resolve(() => {});
  });
  vi.mocked(onNoteUpdateSuggested).mockImplementation((h) => {
    handler = h;
    return Promise.resolve(() => {});
  });

  const view = render(
    <I18nextProvider i18n={testI18n}>
      <NoteSuggestionToast />
    </I18nextProvider>,
  );
  await waitFor(() => expect(handler).not.toBeNull());
  const emit = (event: NoteUpdateSuggestedEvent) => act(() => handler!(event));
  const emitExpired = (actionId: string) =>
    act(() => expiredHandler!({ actionId }));
  return { ...view, emit, emitExpired };
}

function suggestion(summaryIncomplete: boolean): NoteUpdateSuggestedEvent {
  return {
    sessionId: "session-1",
    actionId: "action-1",
    action: {
      ProposeNoteUpdate: { target: "CurrentServer", new_content: "Gekürzte Notiz" },
    },
    previousNoteContent: "Lange alte Notiz",
    targetName: "Server A",
    summaryIncomplete,
  };
}

async function expand() {
  fireEvent.click(await screen.findByRole("button", { name: "Anzeigen" }));
}

describe("NoteSuggestionToast — incomplete summary (Spec 0057, Issue #94)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the incomplete warning above the diff when the flag is set", async () => {
    const { emit } = await renderToastAndCaptureEmit();
    emit(suggestion(true));
    await expand();

    const warning = screen.getByRole("alert");
    expect(warning).toHaveTextContent(WARNING_DE);
    // Über dem Diff: die Warnung steht im DOM vor dem Diff-Inhalt.
    const diffText = screen.getByText(/Gekürzte Notiz/);
    expect(
      warning.compareDocumentPosition(diffText) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("shows no warning when the flag is false", async () => {
    const { emit } = await renderToastAndCaptureEmit();
    emit(suggestion(false));
    await expand();

    expect(screen.getByText(/Gekürzte Notiz/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(WARNING_DE)).toBeNull();
  });

  it("accept and reject work unchanged on a flagged suggestion", async () => {
    const { emit } = await renderToastAndCaptureEmit();
    emit(suggestion(true));
    await expand();
    fireEvent.click(screen.getByRole("button", { name: "Übernehmen" }));
    await waitFor(() =>
      expect(respondToAction).toHaveBeenCalledWith("session-1", "action-1", {
        decision: "approve",
      }),
    );

    emit({ ...suggestion(true), actionId: "action-2" });
    await expand();
    fireEvent.click(screen.getByRole("button", { name: "Ablehnen" }));
    await waitFor(() =>
      expect(respondToAction).toHaveBeenCalledWith("session-1", "action-2", {
        decision: "deny",
      }),
    );
  });

  it("has the warning text in both locales", () => {
    expect(testI18n.getFixedT("de")("noteSuggestion.summaryIncomplete")).toBe(WARNING_DE);
    expect(testI18n.getFixedT("en")("noteSuggestion.summaryIncomplete")).toBe(
      "The summary was cut off and is incomplete. Check it carefully or reject it.",
    );
  });
});

describe("NoteSuggestionToast — close and expired state (Issue #628)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("the expanded card has a close button that rejects and removes the card", async () => {
    const { emit } = await renderToastAndCaptureEmit();
    emit(suggestion(false));
    await expand();

    fireEvent.click(
      screen.getByRole("button", { name: "Vorschlag verwerfen" }),
    );

    await waitFor(() =>
      expect(respondToAction).toHaveBeenCalledWith("session-1", "action-1", {
        decision: "deny",
      }),
    );
    await waitFor(() => expect(screen.queryByText(/Server A/)).toBeNull());
  });

  it("an expiry event switches the card to the expired state with only a close button", async () => {
    const { emit, emitExpired } = await renderToastAndCaptureEmit();
    emit(suggestion(false));
    await expand();
    emitExpired("action-1");

    expect(screen.getByRole("status")).toHaveTextContent(
      "Abgelaufen — nicht angewendet",
    );
    expect(screen.getAllByRole("button")).toHaveLength(1);

    // Closing an expired card only hides it — no backend call.
    fireEvent.click(
      screen.getByRole("button", { name: "Vorschlag verwerfen" }),
    );
    expect(screen.queryByRole("status")).toBeNull();
    expect(respondToAction).not.toHaveBeenCalled();
  });

  it("Apply on an unknown action id switches to expired without an error", async () => {
    vi.mocked(respondToAction).mockRejectedValueOnce({
      code: "ACTION_CONFIRMATION_UNKNOWN",
      message: "keine wartende Bestätigung",
    });
    const { emit } = await renderToastAndCaptureEmit();
    emit(suggestion(false));
    await expand();

    fireEvent.click(screen.getByRole("button", { name: "Übernehmen" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "Abgelaufen — nicht angewendet",
    );
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });
});
