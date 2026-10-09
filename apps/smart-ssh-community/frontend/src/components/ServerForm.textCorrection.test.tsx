// Issue #217: technical fields of the server form switch off the system text
// correction (macOS auto-capitalization etc.); free-text fields keep it.
import { render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { getServer } from "../api";
import { testI18n } from "../testI18n";
import type { ServerDto } from "../types";
import { ServerForm } from "./ServerForm";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
  createServer: vi.fn(),
  deleteServer: vi.fn(),
  getServer: vi.fn(),
  inspectKeyFile: vi.fn(),
  largeNoteDialogThresholdChars: vi.fn(() => Promise.resolve(100000)),
  previewEffectiveNotes: vi.fn(() => Promise.resolve("")),
  requestNoteShrink: vi.fn(),
  testConnection: vi.fn(),
  trustHostKey: vi.fn(),
  updateLocalServerNotes: vi.fn(),
  updateLocalServerTags: vi.fn(),
  updateServer: vi.fn(),
}));
vi.mock("../events", () => ({
  onNoteShrinkSucceeded: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("../fileDialog", () => ({
  pickAndReadTextFile: vi.fn(() => Promise.resolve(null)),
  pickFilePath: vi.fn(() => Promise.resolve(null)),
}));

const SERVER: ServerDto = {
  id: "00000000-0000-0000-0000-000000000217",
  name: "web-01",
  host: "db01.example.lan",
  port: 22,
  username: "root",
  groupId: null,
  tags: [],
  authKind: "agent",
  identityFilePath: null,
  jumpHost: null,
  notes: "",
  hasSudoPassword: false,
  sudoPasswordUnknown: false,
  isLocal: false,
  postIngestPolicy: "balanced",
  aiInjectionCheckEnabled: false,
  sftpServerPath: null,
};

function renderNew() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerForm
        serverId={SERVER.id}
        defaultGroupId={null}
        allGroups={[]}
        allServers={[]}
        onSaved={vi.fn()}
        onDeleted={vi.fn()}
      />
    </I18nextProvider>,
  );
}

describe("ServerForm text correction (issue #217)", () => {
  it("disables correction on host and username", async () => {
    vi.mocked(getServer).mockResolvedValue(SERVER);
    renderNew();
    await waitFor(() => expect(screen.getByDisplayValue("db01.example.lan")).toBeTruthy());
    for (const key of ["serverForm.host", "serverForm.username"]) {
      const field = screen.getByLabelText(testI18n.t(key));
      expect(field.getAttribute("autocapitalize")).toBe("off");
      expect(field.getAttribute("autocorrect")).toBe("off");
      expect(field.getAttribute("spellcheck")).toBe("false");
    }
  });

  it("keeps correction on the notes textarea", async () => {
    vi.mocked(getServer).mockResolvedValue(SERVER);
    const { container } = renderNew();
    await waitFor(() => expect(screen.getByDisplayValue("db01.example.lan")).toBeTruthy());
    const notes = container.querySelector("textarea");
    expect(notes).not.toBeNull();
    expect(notes!.hasAttribute("autocapitalize")).toBe(false);
    expect(notes!.hasAttribute("autocorrect")).toBe(false);
    expect(notes!.hasAttribute("spellcheck")).toBe(false);
  });
});
