// Spec 0102: das Startverzeichnis im Server-Formular — bearbeitbar,
// relative Pfade außer `~/…` werden abgelehnt, und der lokale
// Pseudo-Server bietet das Feld nicht an.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createServer, getServer, updateServer } from "../api";
import { testI18n } from "../testI18n";
import type { ServerDto } from "../types";
import { ServerForm } from "./ServerForm";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
  clearServerSudoPassword: vi.fn(),
  convertIdentityFileToKeychain: vi.fn(),
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

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

const SERVER_ID = "11111111-1111-4111-8111-111111111111";
const LABEL = "Startverzeichnis (Terminal und Dateibrowser)";
const NOT_ABSOLUTE =
  "Das Startverzeichnis muss ein absoluter Pfad (/…) sein oder mit ~/ beginnen.";

function serverDto(overrides: Partial<ServerDto> = {}): ServerDto {
  return {
    id: SERVER_ID,
    name: "web-01",
    host: "example.invalid",
    port: 22,
    username: "deploy",
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
    startDirectory: null,
    ...overrides,
  };
}

function renderForm(serverId: string | null = SERVER_ID) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerForm
        serverId={serverId}
        defaultGroupId={null}
        allGroups={[]}
        allServers={[]}
        onSaved={vi.fn()}
        onDeleted={vi.fn()}
      />
    </I18nextProvider>,
  );
}

describe("ServerForm start directory (Spec 0102)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(updateServer).mockResolvedValue(undefined);
    vi.mocked(createServer).mockResolvedValue(SERVER_ID);
  });

  it("shows the stored value and saves a trimmed new one", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ startDirectory: "/srv/app" }));
    renderForm();

    const input = (await screen.findByLabelText(LABEL)) as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe("/srv/app"));

    fireEvent.change(input, { target: { value: "  ~/it's my dir  " } });
    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));

    await waitFor(() => expect(updateServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(updateServer).mock.calls[0][1].startDirectory).toBe("~/it's my dir");
  });

  it("saves an empty field as not set", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ startDirectory: "/srv/app" }));
    renderForm();

    const input = (await screen.findByLabelText(LABEL)) as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe("/srv/app"));
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));

    await waitFor(() => expect(updateServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(updateServer).mock.calls[0][1].startDirectory).toBeNull();
  });

  it.each(["srv/app", "./app", "~", "~root/app"])(
    "rejects the relative path %s with a clear message and does not save",
    async (value) => {
      vi.mocked(getServer).mockResolvedValue(serverDto());
      renderForm();

      const input = await screen.findByLabelText(LABEL);
      fireEvent.change(input, { target: { value } });

      expect((await screen.findAllByText(NOT_ABSOLUTE)).length).toBeGreaterThan(0);
      fireEvent.click(screen.getByRole("button", { name: "Speichern" }));
      await waitFor(() => expect(screen.getAllByText(NOT_ABSOLUTE).length).toBe(2));
      expect(updateServer).not.toHaveBeenCalled();
    },
  );

  it("is not offered for the local pseudo-server", async () => {
    vi.mocked(getServer).mockResolvedValue(
      serverDto({ id: "00000000-0000-0000-0000-000000000000", isLocal: true, name: "Localhost" }),
    );
    renderForm("00000000-0000-0000-0000-000000000000");

    await waitFor(() => expect(getServer).toHaveBeenCalled());
    // Vor dem Laden ist `isLocal` noch unbekannt; danach verschwindet das Feld.
    await waitFor(() => expect(screen.queryByLabelText(LABEL)).toBeNull());
    expect(screen.queryByText(/Startverzeichnis/)).toBeNull();
  });
});
