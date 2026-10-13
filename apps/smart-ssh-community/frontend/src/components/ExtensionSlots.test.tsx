// Spec 0038, Abschnitt 4: Erweiterungspunkte im Server-Formular, Gruppen-
// Formular und in der Verwalten-Seitenleiste.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getServer, listGroups, listServers } from "../api";
import { registerGroupFormSection, registerManageView, registerServerFormSection, resetRegistryForTests } from "../extensions";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { GroupForm } from "./GroupForm";
import { ManagementView } from "./ManagementView";
import { ServerForm } from "./ServerForm";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
  clearServerSudoPassword: vi.fn(),
  convertIdentityFileToKeychain: vi.fn(),
  createServer: vi.fn(),
  deleteServer: vi.fn(),
  deleteGroup: vi.fn(),
  createGroup: vi.fn(),
  updateGroup: vi.fn(),
  getServer: vi.fn(),
  inspectKeyFile: vi.fn(),
  listGroups: vi.fn(),
  listServers: vi.fn(),
  listUnusableServers: vi.fn(() => Promise.resolve([])),
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
vi.mock("../events", () => ({ onNoteShrinkSucceeded: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../fileDialog", () => ({
  pickAndReadTextFile: vi.fn(() => Promise.resolve(null)),
  pickFilePath: vi.fn(() => Promise.resolve(null)),
}));
vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

function serverDto(overrides: Partial<ServerDto> = {}): ServerDto {
  return {
    id: "s-1",
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
const group: GroupDto = { id: "g-1", name: "Prod", parentId: null, notes: "" };

function ServerSection({ serverId, dirty }: { serverId: string; dirty: boolean }) {
  return <p>server-ext {serverId} {dirty ? "dirty" : "clean"}</p>;
}
function GroupSection({ groupId }: { groupId: string }) {
  return <p>group-ext {groupId}</p>;
}
function ViewBody() {
  return <p>manage-view-body</p>;
}

const wrap = (ui: React.ReactNode) => render(<I18nextProvider i18n={testI18n}>{ui}</I18nextProvider>);

function serverForm(serverId: string | null) {
  return wrap(
    <ServerForm serverId={serverId} defaultGroupId={null} allGroups={[]} allServers={[]} onSaved={vi.fn()} onDeleted={vi.fn()} />,
  );
}

beforeEach(() => {
  vi.mocked(getServer).mockResolvedValue(serverDto());
  vi.mocked(listGroups).mockResolvedValue([group]);
  vi.mocked(listServers).mockResolvedValue([serverDto()]);
});
afterEach(() => resetRegistryForTests());

describe("server form section", () => {
  it("renders for an existing server with the saved/unsaved state", async () => {
    registerServerFormSection({ id: "x", component: ServerSection });
    const { container } = serverForm("s-1");
    expect(await screen.findByText("server-ext s-1 clean")).toBeInTheDocument();
    fireEvent.change(container.querySelector("input") as HTMLInputElement, { target: { value: "changed" } });
    expect(await screen.findByText("server-ext s-1 dirty")).toBeInTheDocument();
  });

  it("renders for the local pseudo-server", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ id: "local", isLocal: true, name: "Localhost" }));
    registerServerFormSection({ id: "x", component: ServerSection });
    serverForm("local");
    expect(await screen.findByText("server-ext local clean")).toBeInTheDocument();
  });

  it("does not render for a new, unsaved server", async () => {
    registerServerFormSection({ id: "x", component: ServerSection });
    serverForm(null);
    await screen.findByRole("button", { name: /create|erstellen|anlegen/i });
    expect(screen.queryByText(/server-ext/)).not.toBeInTheDocument();
  });

  it("appears without reload when registered after mount", async () => {
    serverForm("s-1");
    await waitFor(() => expect(getServer).toHaveBeenCalled());
    await screen.findByDisplayValue("web-01");
    expect(screen.queryByText(/server-ext/)).not.toBeInTheDocument();
    act(() => registerServerFormSection({ id: "late", component: ServerSection }));
    expect(await screen.findByText("server-ext s-1 clean")).toBeInTheDocument();
  });
});

describe("group form section", () => {
  const groupForm = (groupId: string | null) =>
    wrap(<GroupForm groupId={groupId} defaultParentId={null} allGroups={[group]} onSaved={vi.fn()} onDeleted={vi.fn()} />);

  it("renders for an existing group only", () => {
    registerGroupFormSection({ id: "g", component: GroupSection });
    const { unmount } = groupForm("g-1");
    expect(screen.getByText("group-ext g-1")).toBeInTheDocument();
    unmount();
    groupForm(null);
    expect(screen.queryByText(/group-ext/)).not.toBeInTheDocument();
  });

  it("appears without reload when registered after mount", () => {
    groupForm("g-1");
    expect(screen.queryByText(/group-ext/)).not.toBeInTheDocument();
    act(() => registerGroupFormSection({ id: "late", component: GroupSection }));
    expect(screen.getByText("group-ext g-1")).toBeInTheDocument();
  });
});

describe("manage view", () => {
  it("shows an entry, reachable by keyboard, and renders it in the main area", async () => {
    registerManageView({ id: "v", label: "My view", icon: "★", component: ViewBody });
    wrap(<ManagementView />);
    const entry = await screen.findByTestId("extension-view-entry");
    expect(entry).toHaveTextContent("My view");
    entry.focus();
    expect(entry).toHaveFocus();
    fireEvent.keyDown(entry, { key: "Enter" });
    expect(await screen.findByText("manage-view-body")).toBeInTheDocument();
    expect(entry).toHaveAttribute("aria-selected", "true");
  });

  it("appears without reload when registered after mount", async () => {
    wrap(<ManagementView />);
    await screen.findByText(/web-01/);
    expect(screen.queryByTestId("extension-view-entry")).not.toBeInTheDocument();
    act(() => registerManageView({ id: "late", label: "Late", component: ViewBody }));
    expect(await screen.findByTestId("extension-view-entry")).toBeInTheDocument();
  });

  it("adds nothing without registrations", async () => {
    wrap(<ManagementView />);
    await screen.findByText(/web-01/);
    expect(screen.queryByTestId("extension-view-entry")).not.toBeInTheDocument();
  });
});
