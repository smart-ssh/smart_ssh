// Issue #63 / Spec 0103 („Geöffnetes Formular"): Wird das gerade im
// Verwalten-Formular geöffnete Element per Drag-and-drop verschoben, bleiben
// alle ungespeicherten Eingaben stehen; nur das Gruppen- bzw.
// Übergruppen-Feld zeigt den neuen Ort, damit ein späteres Speichern das
// Verschieben nicht rückgängig macht.
//
// *Gegenbeweis:* Mit dem früheren Remount (`moveRevision` im Formular-`key`)
// schlugen die Fälle „verschiebt den geöffneten Server/die geöffnete Gruppe"
// fehl (Name/Host wieder auf dem gespeicherten Wert). Ohne das neue
// `syncedFor`-Gate in `GroupForm` schlug zusätzlich „Verschieben eines
// anderen Elements" für die geöffnete Gruppe fehl (jedes Neuladen setzte den
// Namen zurück). Beides verifiziert, dann wiederhergestellt.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { moveGroup, moveServerToGroup, updateGroup, updateServer } from "../api";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { ManagementView } from "./ManagementView";

function group(id: string, name: string, parentId: string | null): GroupDto {
  return { id, name, parentId, notes: "" };
}

function server(id: string, name: string, groupId: string | null): ServerDto {
  return {
    id,
    name,
    host: `${name}.example.invalid`,
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

/** Der „Backend"-Zustand: die Move-Befehle ändern ihn, `listGroups`/
 * `listServers`/`getServer` lesen ihn — wie das echte Neuladen nach einem
 * Verschieben. */
const backend = vi.hoisted(() => ({
  groups: [] as GroupDto[],
  servers: [] as ServerDto[],
}));

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: (err: unknown) =>
    err && typeof err === "object" && "code" in err ? (err as { code: string }).code : null,
  listUnusableServers: vi.fn(() => Promise.resolve([])),
  listGroups: vi.fn(() => Promise.resolve(backend.groups.map((g) => ({ ...g })))),
  listServers: vi.fn(() => Promise.resolve(backend.servers.map((s) => ({ ...s })))),
  listStoredHostKeys: vi.fn(() => Promise.resolve([])),
  getServer: vi.fn((id: string) => {
    const found = backend.servers.find((s) => s.id === id);
    return found ? Promise.resolve({ ...found }) : Promise.reject(new Error("not found"));
  }),
  moveServerToGroup: vi.fn((id: string, groupId: string | null) => {
    backend.servers = backend.servers.map((s) => (s.id === id ? { ...s, groupId } : s));
    return Promise.resolve();
  }),
  moveGroup: vi.fn((id: string, parentId: string | null) => {
    backend.groups = backend.groups.map((g) => (g.id === id ? { ...g, parentId } : g));
    return Promise.resolve();
  }),
  createServer: vi.fn(),
  updateServer: vi.fn(() => Promise.resolve()),
  deleteServer: vi.fn(),
  createGroup: vi.fn(),
  updateGroup: vi.fn(() => Promise.resolve()),
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

function renderView() {
  void testI18n.changeLanguage("en");
  return render(
    <I18nextProvider i18n={testI18n}>
      <ManagementView />
    </I18nextProvider>,
  );
}

/** Eine Zeile der Sidebar (nicht die gleichlautende Formular-Überschrift). */
function sidebarRow(text: string): HTMLElement {
  const row = screen
    .getAllByText(new RegExp(`^\\S+ ${text}$`))
    .find((el) => el.closest("[data-drop-target]") !== null);
  if (!row) throw new Error(`sidebar row ${text} not found`);
  return row;
}

/** Ein vollständiges Ziehen in der Sidebar (s. `Sidebar.test.tsx`). */
async function dragOnto(source: HTMLElement, over: HTMLElement) {
  document.elementFromPoint = vi.fn(() => over);
  await act(async () => {
    fireEvent.pointerDown(source, { button: 0, buttons: 1, pointerId: 1, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(source, { buttons: 1, pointerId: 1, clientX: 40, clientY: 60 });
    fireEvent.pointerUp(source, { button: 0, buttons: 0, pointerId: 1, clientX: 40, clientY: 60 });
    fireEvent.click(source);
  });
}

/** Das Gruppen-/Übergruppen-Dropdown — das einzige mit „Ops" als Option. */
function groupSelect(): HTMLSelectElement {
  const select = screen
    .getAllByRole("combobox")
    .find((el) => el.querySelector('option[value="g-ops"]') !== null);
  if (!select) throw new Error("group select not found");
  return select as HTMLSelectElement;
}

const nameInput = () => screen.getByLabelText(testI18n.t("common.name")) as HTMLInputElement;
const hostInput = () => screen.getByLabelText(testI18n.t("serverForm.host")) as HTMLInputElement;
const submitForm = async () => {
  await act(async () => {
    fireEvent.submit(nameInput().closest("form")!);
  });
};

async function openServer(name: string) {
  renderView();
  await screen.findByText(/web-1$/);
  fireEvent.click(sidebarRow(name));
  await waitFor(() => expect(nameInput().value).toBe(name));
}

async function openGroup(name: string) {
  renderView();
  await screen.findByText(/web-1$/);
  fireEvent.click(sidebarRow(name));
  await waitFor(() => expect(nameInput().value).toBe(name));
}

describe("ManagementView: moving the open item keeps unsaved edits (issue #63)", () => {
  beforeEach(() => {
    backend.groups = [
      group("g-prod", "Prod", null),
      group("g-web", "Web", "g-prod"),
      group("g-ops", "Ops", null),
    ];
    backend.servers = [server("s-web", "web-1", "g-web"), server("s-free", "free-1", null)];
    vi.clearAllMocks();
  });

  afterEach(() => {
    // @ts-expect-error -- jsdom hat die Funktion von Haus aus nicht.
    delete document.elementFromPoint;
    void testI18n.changeLanguage("de");
  });

  it("server moved into another folder: edits survive, group shows the new folder, save keeps both", async () => {
    await openServer("web-1");
    fireEvent.change(nameInput(), { target: { value: "web-renamed" } });
    fireEvent.change(hostInput(), { target: { value: "new.example.invalid" } });

    await dragOnto(sidebarRow("web-1"), sidebarRow("Ops"));

    expect(moveServerToGroup).toHaveBeenCalledWith("s-web", "g-ops");
    await waitFor(() => expect(groupSelect().value).toBe("g-ops"));
    expect(nameInput().value).toBe("web-renamed");
    expect(hostInput().value).toBe("new.example.invalid");

    await submitForm();
    expect(updateServer).toHaveBeenCalledWith(
      "s-web",
      expect.objectContaining({ name: "web-renamed", host: "new.example.invalid", groupId: "g-ops" }),
    );
  });

  it("server moved to the root: edits survive, group shows none, save keeps the root", async () => {
    await openServer("web-1");
    fireEvent.change(nameInput(), { target: { value: "web-renamed" } });

    await dragOnto(sidebarRow("web-1"), sidebarRow("free-1"));

    expect(moveServerToGroup).toHaveBeenCalledWith("s-web", null);
    await waitFor(() => expect(groupSelect().value).toBe(""));
    expect(nameInput().value).toBe("web-renamed");

    await submitForm();
    expect(updateServer).toHaveBeenCalledWith(
      "s-web",
      expect.objectContaining({ name: "web-renamed", groupId: null }),
    );
  });

  it("folder moved under another folder: edits survive, parent shows the new parent, save keeps it", async () => {
    await openGroup("Web");
    fireEvent.change(nameInput(), { target: { value: "Web renamed" } });

    await dragOnto(sidebarRow("Web"), sidebarRow("Ops"));

    expect(moveGroup).toHaveBeenCalledWith("g-web", "g-ops");
    await waitFor(() => expect(groupSelect().value).toBe("g-ops"));
    expect(nameInput().value).toBe("Web renamed");

    await submitForm();
    expect(updateGroup).toHaveBeenCalledWith("g-web", "Web renamed", "g-ops");
  });

  it("folder moved to the root: edits survive, parent shows none, save keeps the root", async () => {
    await openGroup("Web");
    fireEvent.change(nameInput(), { target: { value: "Web renamed" } });

    await dragOnto(sidebarRow("Web"), sidebarRow("free-1"));

    expect(moveGroup).toHaveBeenCalledWith("g-web", null);
    await waitFor(() => expect(groupSelect().value).toBe(""));
    expect(nameInput().value).toBe("Web renamed");

    await submitForm();
    expect(updateGroup).toHaveBeenCalledWith("g-web", "Web renamed", null);
  });

  it("a cycle-rejected folder move leaves the form unchanged and shows the translated error", async () => {
    vi.mocked(moveGroup).mockRejectedValueOnce({ code: "GROUP_CYCLE_DETECTED", message: "cycle" });
    await openGroup("Prod");
    fireEvent.change(nameInput(), { target: { value: "Prod renamed" } });
    fireEvent.change(groupSelect(), { target: { value: "g-ops" } });

    await dragOnto(sidebarRow("Prod"), sidebarRow("Web"));

    expect(moveGroup).toHaveBeenCalledWith("g-prod", "g-web");
    expect(
      await screen.findByText(testI18n.t("errors.GROUP_CYCLE_DETECTED")),
    ).toBeInTheDocument();
    expect(nameInput().value).toBe("Prod renamed");
    expect(groupSelect().value).toBe("g-ops");
  });

  it("moving another item leaves an open server form untouched", async () => {
    await openServer("web-1");
    fireEvent.change(nameInput(), { target: { value: "web-renamed" } });

    await dragOnto(sidebarRow("free-1"), sidebarRow("Ops"));

    expect(moveServerToGroup).toHaveBeenCalledWith("s-free", "g-ops");
    // Das Neuladen der Listen nach dem Verschieben ist durch.
    await waitFor(() => expect(sidebarRow("free-1").closest('[data-drop-target="group:g-ops"]')).not.toBeNull());
    expect(nameInput().value).toBe("web-renamed");
    expect(groupSelect().value).toBe("g-web");
  });

  it("moving another item leaves an open folder form untouched", async () => {
    await openGroup("Web");
    fireEvent.change(nameInput(), { target: { value: "Web renamed" } });
    fireEvent.change(groupSelect(), { target: { value: "g-ops" } });

    await dragOnto(sidebarRow("free-1"), sidebarRow("Prod"));

    expect(moveServerToGroup).toHaveBeenCalledWith("s-free", "g-prod");
    await waitFor(() => expect(sidebarRow("free-1").closest('[data-drop-target="group:g-prod"]')).not.toBeNull());
    expect(nameInput().value).toBe("Web renamed");
    expect(groupSelect().value).toBe("g-ops");
  });
});
