// Issue #177 / Spec 0008 §6a: Die Löschvorschau einer Gruppe nennt auch
// nicht nutzbare Server, die ihre Gruppenzuordnung verlieren.
//
// *Gegenbeweis:* Ohne die `unusableServersToUnassign`-Zeilen in `GroupForm`
// fehlt der Name des nicht nutzbaren Servers, und bei sonst leerer Vorschau
// erscheint fälschlich „keine Auswirkungen".
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { deleteGroup } from "../api";
import { testI18n } from "../testI18n";
import type { DeleteGroupResult } from "../types";
import { GroupForm } from "./GroupForm";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
  createGroup: vi.fn(),
  updateGroup: vi.fn(),
  deleteGroup: vi.fn(),
  largeNoteDialogThresholdChars: vi.fn(() => Promise.resolve(10000)),
}));

function renderForm() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <GroupForm
        groupId="g1"
        defaultParentId={null}
        allGroups={[{ id: "g1", name: "Produktion", parentId: null, notes: "" }]}
        onSaved={() => {}}
        onDeleted={() => {}}
      />
    </I18nextProvider>,
  );
}

describe("GroupForm delete preview", () => {
  it("lists a not usable server that loses its group", async () => {
    const preview: DeleteGroupResult = {
      childGroupsToDelete: [],
      serversToUnassign: [],
      unusableServersToUnassign: [
        { id: "u1", name: "kaputt", host: "h", groupId: "g1", reason: "unknown_auth_method" },
      ],
      executed: false,
    };
    vi.mocked(deleteGroup).mockResolvedValue(preview);
    renderForm();

    fireEvent.click(screen.getByRole("button", { name: "Gruppe löschen" }));

    await waitFor(() => expect(screen.getByText(/kaputt/)).toBeTruthy());
    expect(screen.getByText(/Nicht nutzbarer Server/)).toBeTruthy();
    expect(screen.queryByText(/Keine weiteren Objekte/)).toBeNull();
  });
});
