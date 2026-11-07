// Issue #1194 / Spec 0067: the per-server switch for elevated rights.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createServer, getServer, updateServer } from "../api/api";
import { testI18n } from "../testI18n";
import type { ServerDto } from "../api/types";
import { ServerForm } from "./ServerForm";

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    clearServerSudoPassword: vi.fn(),
    convertIdentityFileToKeychain: vi.fn(),
    createServer: vi.fn(),
    deleteServer: vi.fn(),
    getServer: vi.fn(),
    listStoredHostKeys: vi.fn(() => Promise.resolve([])),
    inspectKeyFile: vi.fn(),
    largeNoteDialogThresholdChars: vi.fn(() => Promise.resolve(100000)),
    previewEffectiveNotes: vi.fn(() => Promise.resolve("")),
    requestNoteShrink: vi.fn(),
    testConnection: vi.fn(),
    trustHostKey: vi.fn(),
    updateLocalServerNotes: vi.fn(),
    updateLocalServerTags: vi.fn(),
    updateServer: vi.fn(),
  }),
);

vi.mock("../api/events", () => ({
  onNoteShrinkSucceeded: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../api/fileDialog", () => ({
  pickAndReadTextFile: vi.fn(() => Promise.resolve(null)),
  pickFilePath: vi.fn(() => Promise.resolve(null)),
}));

vi.mock("../settings/riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

const SERVER_ID = "11111111-1111-4111-8111-111111111111";
const SWITCH = "Erhöhte Rechte im Dateibrowser erlauben";
const PATH = "Pfad zu sftp-server (erhöhter Dateizugriff)";

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
    elevationAllowed: true,
    isCopy: false,
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

describe("ServerForm elevated rights switch (issue #1194)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(updateServer).mockResolvedValue(undefined);
    vi.mocked(createServer).mockResolvedValue(SERVER_ID);
  });

  it("is on for a new server and saved as off after unchecking", async () => {
    renderForm(null);
    const box = (await screen.findByLabelText(SWITCH)) as HTMLInputElement;
    expect(box.checked).toBe(true);
    fireEvent.click(box);
    await waitFor(() => expect(box.checked).toBe(false));
    fireEvent.change(screen.getByLabelText(/^Name/), { target: { value: "web-02" } });
    fireEvent.change(screen.getByLabelText(/^Host/), { target: { value: "h.invalid" } });
    fireEvent.change(screen.getByLabelText(/^Benutzer/), { target: { value: "me" } });
    fireEvent.click(screen.getByRole("button", { name: "Anlegen" }));
    await waitFor(() => expect(createServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(createServer).mock.calls[0][0].elevationAllowed).toBe(false);
  });

  it("loads the stored off state and disables the sftp-server path field", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ elevationAllowed: false }));
    renderForm();
    const box = (await screen.findByLabelText(SWITCH)) as HTMLInputElement;
    await waitFor(() => expect(box.checked).toBe(false));
    expect(screen.getByLabelText(PATH)).toBeDisabled();
    fireEvent.click(box);
    await waitFor(() => expect(screen.getByLabelText(PATH)).toBeEnabled());
  });

  it("saves an existing server with the switch off", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto());
    renderForm();
    const box = (await screen.findByLabelText(SWITCH)) as HTMLInputElement;
    await waitFor(() => expect(box.checked).toBe(true));
    fireEvent.click(box);
    await waitFor(() => expect(box.checked).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));
    await waitFor(() => expect(updateServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(updateServer).mock.calls[0][1].elevationAllowed).toBe(false);
  });
});
