// Issue #100: Ein Server, dessen Anmeldeart diese Version nicht lesen kann,
// zeigt seine Begründung und bietet nur das Löschen an — zweistufig, und
// ein nicht entferntes Secret wird genannt statt still verschluckt.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { deleteUnusableServer } from "../api";
import { testI18n } from "../testI18n";
import type { DeleteUnusableServerResult, UnusableServerDto } from "../types";
import { UnusableServerPanel } from "./UnusableServerPanel";

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return {
    deleteUnusableServer: vi.fn(),
    commandErrorMessage: actual.commandErrorMessage,
    commandErrorCode: actual.commandErrorCode,
  };
});

const SERVER: UnusableServerDto = {
  id: "unusable-1",
  name: "newer-box",
  host: "newer.internal",
  groupId: null,
  reason: "unreadable_auth_method",
};

function result(overrides: Partial<DeleteUnusableServerResult> = {}): DeleteUnusableServerResult {
  return {
    server: SERVER,
    serversLosingJumpHost: [],
    executed: false,
    secretsLeftBehind: [],
    ...overrides,
  };
}

function renderPanel(onDeleted = vi.fn()) {
  render(
    <I18nextProvider i18n={testI18n}>
      <UnusableServerPanel server={SERVER} onDeleted={onDeleted} />
    </I18nextProvider>,
  );
  return onDeleted;
}

describe("UnusableServerPanel", () => {
  beforeEach(() => vi.mocked(deleteUnusableServer).mockReset());

  it("shows the reason and offers no edit or connect action", () => {
    renderPanel();
    expect(screen.getByText("Nicht nutzbar")).toBeInTheDocument();
    expect(
      screen.getByText("Die gespeicherte Anmeldeart ist beschädigt und nicht lesbar."),
    ).toBeInTheDocument();
    // Only the delete button exists — no save, test or connect.
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["Server löschen"]);
  });

  it("deletes only after confirmation", async () => {
    vi.mocked(deleteUnusableServer)
      .mockResolvedValueOnce(result())
      .mockResolvedValueOnce(result({ executed: true }));
    const onDeleted = renderPanel();

    fireEvent.click(screen.getByText("Server löschen"));
    await screen.findByText("Alle für diesen Server gespeicherten Secrets werden gelöscht.");
    expect(deleteUnusableServer).toHaveBeenCalledWith("unusable-1", false);
    expect(onDeleted).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Endgültig löschen"));
    await waitFor(() => expect(onDeleted).toHaveBeenCalled());
    expect(deleteUnusableServer).toHaveBeenLastCalledWith("unusable-1", true);
  });

  it("names every secret that could not be removed", async () => {
    vi.mocked(deleteUnusableServer)
      .mockResolvedValueOnce(result())
      .mockResolvedValueOnce(
        result({ executed: true, secretsLeftBehind: ["server:unusable-1:password"] }),
      );
    const onDeleted = renderPanel();

    fireEvent.click(screen.getByText("Server löschen"));
    fireEvent.click(await screen.findByText("Endgültig löschen"));

    const leftovers = await screen.findByTestId("secrets-left-behind");
    expect(leftovers).toHaveTextContent("server:unusable-1:password");
    expect(onDeleted).not.toHaveBeenCalled();
  });
});
