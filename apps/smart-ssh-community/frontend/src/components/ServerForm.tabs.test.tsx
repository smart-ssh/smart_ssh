// Issue #606: the server form is split into General, Connection, Security
// and Notes tabs (Spec 0008). One draft, one Save.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createServer, getServer, updateServer } from "../api";
import { testI18n } from "../testI18n";
import type { ServerDto } from "../types";
import { ServerForm } from "./ServerForm";

vi.mock("../api", () => ({
  // Spec 0069, Teil B (echte `code`-Extraktion für `runOllamaProbe`,
  // s. `ServerList.test.tsx`) und Spec 0071 (echte `.message`-Extraktion
  // für `KEYCHAIN_UNAVAILABLE`-artige Fehlerobjekte) — echte
  // Implementierungen statt einer vereinfachten Attrappe.
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err
      ? String((err as { message: unknown }).message)
      : String(err),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err
      ? ((err as { code: string | null }).code ?? null)
      : null,
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
    ...overrides,
  };
}

const tabNames = () => screen.getAllByRole("tab").map((t) => t.textContent?.trim());
const openTab = (name: string) => fireEvent.click(screen.getByRole("tab", { name }));
const panelOf = (el: HTMLElement) => el.closest('[role="tabpanel"]') as HTMLElement;

function renderForm(serverId: string | null = SERVER_ID, extra: Partial<Parameters<typeof ServerForm>[0]> = {}) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerForm
        serverId={serverId}
        defaultGroupId={null}
        allGroups={[]}
        allServers={[]}
        onSaved={vi.fn()}
        onDeleted={vi.fn()}
        {...extra}
      />
    </I18nextProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getServer).mockResolvedValue(serverDto());
  vi.mocked(updateServer).mockResolvedValue(undefined);
  vi.mocked(createServer).mockResolvedValue(SERVER_ID);
});

describe("ServerForm tabs (issue #606)", () => {
  it("opens an existing server on General with four tabs and each field in its own tab", async () => {
    renderForm();
    expect(await screen.findByDisplayValue("web-01")).toBeInTheDocument();

    expect(tabNames()).toEqual(["Allgemein", "Verbindung", "Sicherheit", "Notizen"]);
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveAttribute("aria-selected", "true");

    const inTab = (el: HTMLElement) => panelOf(el).getAttribute("id")?.split("-panel-")[1];
    expect(inTab(screen.getByDisplayValue("web-01"))).toBe("general");
    expect(inTab(screen.getByLabelText("Gruppe"))).toBe("general");
    expect(inTab(screen.getByDisplayValue("example.invalid"))).toBe("connection");
    expect(inTab(screen.getByDisplayValue("deploy"))).toBe("connection");
    expect(inTab(screen.getByRole("button", { name: "Verbindung testen", hidden: true }))).toBe("connection");
    expect(inTab(screen.getByRole("radio", { name: /Streng/, hidden: true }))).toBe("security");
    expect(screen.getByDisplayValue("web-01")).toBeVisible();
    expect(screen.getByDisplayValue("example.invalid")).not.toBeVisible();
    expect(screen.getByRole("button", { name: "Server löschen" })).toBeVisible();
  });

  it("keeps unsaved edits across tab switches and saves them with one updateServer call", async () => {
    renderForm();
    expect(await screen.findByDisplayValue("web-01")).toBeInTheDocument();

    fireEvent.change(screen.getByDisplayValue("web-01"), { target: { value: "web-02" } });
    openTab("Verbindung");
    fireEvent.change(screen.getByDisplayValue("example.invalid"), { target: { value: "other.invalid" } });
    openTab("Sicherheit");
    openTab("Allgemein");
    expect(screen.getByDisplayValue("web-02")).toBeInTheDocument();
    openTab("Verbindung");
    expect(screen.getByDisplayValue("other.invalid")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));
    await waitFor(() => expect(updateServer).toHaveBeenCalledTimes(1));
    expect(vi.mocked(updateServer).mock.calls[0][1]).toMatchObject({
      name: "web-02",
      host: "other.invalid",
    });
  });

  it("shows Save on General, Connection and Security but not on Notes", async () => {
    renderForm();
    expect(await screen.findByDisplayValue("web-01")).toBeInTheDocument();
    for (const name of ["Allgemein", "Verbindung", "Sicherheit"]) {
      openTab(name);
      expect(screen.getByRole("button", { name: "Speichern" })).toBeInTheDocument();
    }
    openTab("Notizen");
    expect(screen.queryByRole("button", { name: "Speichern" })).toBeNull();
  });

  it("marks the Connection tab and jumps to it when Save finds an invalid field", async () => {
    renderForm();
    expect(await screen.findByDisplayValue("web-01")).toBeInTheDocument();

    fireEvent.change(screen.getByDisplayValue("example.invalid"), { target: { value: "" } });
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("button", { name: "Speichern" }));

    await waitFor(() =>
      expect(screen.getByRole("tab", { name: /Verbindung/ })).toHaveAttribute("aria-selected", "true"),
    );
    expect(screen.getByRole("tab", { name: /Verbindung/ })).toHaveTextContent("Enthält Fehler");
    expect(screen.getByRole("tab", { name: /Allgemein/ })).not.toHaveTextContent("Enthält Fehler");
    expect(updateServer).not.toHaveBeenCalled();

    // Fixing the field clears the mark.
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "x.invalid" } });
    expect(screen.getByRole("tab", { name: /Verbindung/ })).not.toHaveTextContent("Enthält Fehler");
  });

  it("moves between tabs with the arrow keys, Home and End", async () => {
    renderForm();
    expect(await screen.findByDisplayValue("web-01")).toBeInTheDocument();
    const general = screen.getByRole("tab", { name: "Allgemein" });
    general.focus();
    fireEvent.keyDown(general, { key: "ArrowRight" });
    const connection = screen.getByRole("tab", { name: "Verbindung" });
    expect(connection).toHaveAttribute("aria-selected", "true");
    expect(connection).toHaveFocus();
    fireEvent.keyDown(connection, { key: "End" });
    expect(screen.getByRole("tab", { name: "Notizen" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("tab", { name: "Notizen" }), { key: "Home" });
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("tab", { name: "Allgemein" }), { key: "ArrowLeft" });
    expect(screen.getByRole("tab", { name: "Notizen" })).toHaveAttribute("aria-selected", "true");
  });

  it("shows General, Connection and Security for a new server", () => {
    renderForm(null);
    expect(tabNames()).toEqual(["Allgemein", "Verbindung", "Sicherheit"]);
  });

  it("shows General and Notes only for the local pseudo-server", async () => {
    vi.mocked(getServer).mockResolvedValue(
      serverDto({ id: "00000000-0000-0000-0000-000000000000", isLocal: true, name: "Localhost" }),
    );
    renderForm("00000000-0000-0000-0000-000000000000");
    await waitFor(() => expect(tabNames()).toEqual(["Allgemein", "Notizen"]));
  });

  it("opens General again when another server is selected", async () => {
    const { rerender } = renderForm();
    expect(await screen.findByDisplayValue("web-01")).toBeInTheDocument();
    openTab("Sicherheit");
    rerender(
      <I18nextProvider i18n={testI18n}>
        <ServerForm
          serverId="22222222-2222-4222-8222-222222222222"
          defaultGroupId={null}
          allGroups={[]}
          allServers={[]}
          onSaved={vi.fn()}
          onDeleted={vi.fn()}
        />
      </I18nextProvider>,
    );
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveAttribute("aria-selected", "true");
  });
});
