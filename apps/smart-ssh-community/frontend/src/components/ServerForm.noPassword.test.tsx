// Issue #632: the password is optional. An empty field stores "no password"
// (explicit input), "empty = unchanged" only exists for a saved password, and
// "Remove saved password" is its own input — never an empty string.
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
  listStoredHostKeys: vi.fn(() => Promise.resolve([])),
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

function serverDto(overrides: Partial<ServerDto> = {}): ServerDto {
  return {
    id: SERVER_ID,
    name: "web-01",
    host: "example.invalid",
    port: 22,
    username: "deploy",
    groupId: null,
    tags: [],
    authKind: "password",
    identityFilePath: null,
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    sudoPasswordUnknown: false,
    isLocal: false,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
    sftpServerPath: null,
    ...overrides,
  };
}

function renderForm(serverId: string | null) {
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

const openTab = (name: string) => fireEvent.click(screen.getByRole("tab", { name }));
const passwordField = () => screen.getByLabelText(/^Passwort/, { selector: "input[type=password]" });

async function loaded() {
  expect(await screen.findByDisplayValue("web-01")).toBeInTheDocument();
  openTab("Verbindung");
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(updateServer).mockResolvedValue(undefined);
  vi.mocked(createServer).mockResolvedValue(SERVER_ID);
});

describe("ServerForm — optional password (issue #632)", () => {
  it("creates a server with an empty password as explicit noPassword", async () => {
    renderForm(null);
    fireEvent.change(screen.getByLabelText(/^Name/), { target: { value: "web-01" } });
    openTab("Verbindung");
    fireEvent.change(screen.getByLabelText(/^Host/), { target: { value: "example.invalid" } });
    fireEvent.change(screen.getByLabelText(/^Benutzername/), { target: { value: "deploy" } });
    expect(passwordField()).not.toBeRequired();

    fireEvent.click(screen.getByRole("button", { name: "Anlegen" }));
    await waitFor(() => expect(createServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(createServer).mock.calls[0][0].auth).toEqual({ kind: "noPassword" });
  });

  it("keeps a saved password when the field stays empty (value: null)", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ authKind: "password" }));
    renderForm(SERVER_ID);
    await loaded();

    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));
    await waitFor(() => expect(updateServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(updateServer).mock.calls[0][1].auth).toEqual({ kind: "password", value: null });
  });

  it("'Remove saved password' sends noPassword and disables the field", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ authKind: "password" }));
    renderForm(SERVER_ID);
    await loaded();

    fireEvent.click(screen.getByRole("checkbox", { name: "Gespeichertes Passwort entfernen" }));
    expect(passwordField()).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));
    await waitFor(() => expect(updateServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(updateServer).mock.calls[0][1].auth).toEqual({ kind: "noPassword" });
  });

  it("shows 'no password' for such a server and offers no removal", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ authKind: "no_password" }));
    renderForm(SERVER_ID);
    await loaded();

    expect(screen.getByText(/Kein Passwort gespeichert/)).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "Gespeichertes Passwort entfernen" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));
    await waitFor(() => expect(updateServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(updateServer).mock.calls[0][1].auth).toEqual({ kind: "noPassword" });
  });

  it("sends a typed password as before", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ authKind: "no_password" }));
    renderForm(SERVER_ID);
    await loaded();

    fireEvent.change(passwordField(), { target: { value: "s3cret" } });
    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));
    await waitFor(() => expect(updateServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(updateServer).mock.calls[0][1].auth).toEqual({ kind: "password", value: "s3cret" });
  });
});
