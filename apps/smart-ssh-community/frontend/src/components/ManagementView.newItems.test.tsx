// Issue #49: neue Server/Gruppen landen in der aktuellen Gruppe. Prüft den
// ganzen Weg Sidebar → `ManagementView` → Formular: die Vorgabe steht
// tatsächlich im Gruppen-Dropdown, das Dropdown zeigt die Hierarchie, die
// Vorgabe bleibt änderbar, und ein offenes Neu-Formular verliert seine
// Eingaben nur, wenn der Nutzer ein neues „+" startet.
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getServer } from "../api";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { ManagementView } from "./ManagementView";
import type { Selection } from "./Sidebar";

function group(id: string, name: string, parentId: string | null): GroupDto {
  return { id, name, parentId, notes: "" };
}

function server(id: string, name: string, groupId: string | null): ServerDto {
  return {
    id,
    name,
    host: "example.invalid",
    port: 22,
    username: "user",
    groupId,
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
}

const GROUPS = [group("g-prod", "Prod", null), group("g-web", "Web", "g-prod")];
const SERVERS = [server("s-web", "web-1", "g-web")];

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
  listUnusableServers: vi.fn(() => Promise.resolve([])),
  listGroups: vi.fn(() => Promise.resolve(GROUPS)),
  listServers: vi.fn(() => Promise.resolve(SERVERS)),
  getServer: vi.fn(),
  createServer: vi.fn(),
  updateServer: vi.fn(),
  deleteServer: vi.fn(),
  createGroup: vi.fn(),
  updateGroup: vi.fn(),
  deleteGroup: vi.fn(),
  clearServerSudoPassword: vi.fn(),
  convertIdentityFileToKeychain: vi.fn(),
  inspectKeyFile: vi.fn(() => new Promise(() => {})),
  largeNoteDialogThresholdChars: vi.fn(() => Promise.resolve(100000)),
  previewEffectiveNotes: vi.fn(() => Promise.resolve("")),
  requestNoteShrink: vi.fn(),
  testConnection: vi.fn(),
  trustHostKey: vi.fn(),
  updateLocalServerNotes: vi.fn(),
  updateLocalServerTags: vi.fn(),
  listNoteRevisions: vi.fn(() => Promise.resolve([])),
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

function renderView(initialSelection: Selection | null = null) {
  void testI18n.changeLanguage("en");
  return render(
    <I18nextProvider i18n={testI18n}>
      <ManagementView initialSelection={initialSelection} />
    </I18nextProvider>,
  );
}

/** Das Gruppen-Dropdown des Formulars (Server: „Group", Gruppe: „Parent
 * Group") — das einzige `<select>` mit den Gruppen als Optionen. */
function groupSelect(): HTMLSelectElement {
  const select = screen
    .getAllByRole("combobox")
    .find((el) => el.querySelector('option[value="g-prod"]') !== null);
  if (!select) throw new Error("group select not found");
  return select as HTMLSelectElement;
}

const sidebarRow = (text: string) => screen.getByText(new RegExp(`^\\S+ ${text}$`));
const addServer = () =>
  fireEvent.click(screen.getByRole("button", { name: testI18n.t("sidebar.addServer") }));
const addGroup = () =>
  fireEvent.click(screen.getByRole("button", { name: testI18n.t("sidebar.addGroup") }));
const nameInput = () => screen.getByLabelText(testI18n.t("common.name")) as HTMLInputElement;

describe("ManagementView: new items in the current folder (issue #49)", () => {
  afterEach(() => {
    void testI18n.changeLanguage("de");
  });

  it("+ Server with a folder selected preselects that folder, shown with its path", async () => {
    renderView();
    await screen.findByText(/Web$/);

    fireEvent.click(sidebarRow("Web"));
    addServer();

    await screen.findByText(testI18n.t("serverForm.titleNew"));
    const select = groupSelect();
    expect(select.value).toBe("g-web");
    expect(within(select).getByRole("option", { selected: true }).textContent).toBe(
      "  Prod / Web",
    );
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual([
      testI18n.t("serverForm.noGroup"),
      "Prod",
      "  Prod / Web",
    ]);

    // Nur eine Vorgabe — vor dem Speichern änderbar.
    fireEvent.change(select, { target: { value: "g-prod" } });
    expect(groupSelect().value).toBe("g-prod");
  });

  it("+ Server with a server selected preselects that server's folder", async () => {
    vi.mocked(getServer).mockResolvedValue(SERVERS[0]);
    renderView();
    await screen.findByText(/web-1$/);

    fireEvent.click(sidebarRow("web-1"));
    addServer();

    await screen.findByText(testI18n.t("serverForm.titleNew"));
    expect(groupSelect().value).toBe("g-web");
  });

  it("+ Server and + Group with nothing selected preselect no folder", async () => {
    renderView();
    await screen.findByText(/Web$/);

    addServer();
    await screen.findByText(testI18n.t("serverForm.titleNew"));
    expect(groupSelect().value).toBe("");

    // Ein offenes Neu-Formular trägt seine (leere) Vorgabe weiter.
    addGroup();
    await screen.findByText(testI18n.t("groupForm.titleNew"));
    expect(groupSelect().value).toBe("");
  });

  it("+ Group with a folder selected preselects it as the parent", async () => {
    renderView();
    await screen.findByText(/Web$/);

    fireEvent.click(sidebarRow("Prod"));
    addGroup();

    await screen.findByText(testI18n.t("groupForm.titleNew"));
    expect(groupSelect().value).toBe("g-prod");
  });

  it("opens the form with the folder from the Connect list's + action", async () => {
    renderView({ kind: "newServer", groupId: "g-web" });

    await screen.findByText(testI18n.t("serverForm.titleNew"));
    await waitFor(() => expect(groupSelect().value).toBe("g-web"));
  });

  it("keeps typed input until the user starts a new + Server", async () => {
    const { rerender } = renderView();
    await screen.findByText(/Web$/);

    fireEvent.click(sidebarRow("Web"));
    addServer();
    await screen.findByText(testI18n.t("serverForm.titleNew"));
    fireEvent.change(nameInput(), { target: { value: "typed-name" } });

    // Neu-Rendern (z. B. nach einem Neuladen der Listen) behält die Eingabe.
    await act(async () => {
      rerender(
        <I18nextProvider i18n={testI18n}>
          <ManagementView initialSelection={null} />
        </I18nextProvider>,
      );
    });
    expect(nameInput().value).toBe("typed-name");
    expect(groupSelect().value).toBe("g-web");

    // Ein neues „+ Server" beginnt ein frisches Formular, in derselben Gruppe.
    addServer();
    await waitFor(() => expect(nameInput().value).toBe(""));
    expect(groupSelect().value).toBe("g-web");
  });
});
