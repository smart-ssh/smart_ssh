// Spec 0103: servers and groups moved into "Previous data" by the backup
// import carry a "Copy" badge; "Keep as regular entry" clears the marker
// (a group: after a confirmation that names the counts).
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { ServerList } from "./ServerList";
import { keepGroupAsRegularEntry, keepServerAsRegularEntry, listGroups, listServers } from "../api";

const KEEP = testI18n.t("copyMarker.keep");

function server(id: string, name: string, extra: Partial<ServerDto> = {}): ServerDto {
  return {
    id,
    name,
    host: `${name}.example`,
    port: 22,
    username: "u",
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
    ...extra,
  };
}
const grp = (id: string, name: string, isCopy: boolean): GroupDto => ({
  id,
  name,
  parentId: null,
  notes: "",
  isCopy,
});

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return {
    listServers: vi.fn(),
    listGroups: vi.fn(),
    listUnusableServers: vi.fn(() => Promise.resolve([])),
    listChatSessions: vi.fn(() => Promise.resolve([])),
    keepServerAsRegularEntry: vi.fn(() => Promise.resolve()),
    keepGroupAsRegularEntry: vi.fn((_id: string, confirm: boolean) =>
      Promise.resolve({ applied: confirm, groups: 2, servers: 3 }),
    ),
    commandErrorMessage: actual.commandErrorMessage,
    commandErrorCode: actual.commandErrorCode,
    commandErrorConnectLog: actual.commandErrorConnectLog,
  };
});
vi.mock("../events", () => ({
  onHostKeyVerificationNeeded: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("../firstRunNotice", () => ({
  loadFirstRunNoticeAcknowledged: vi.fn(() => Promise.resolve(true)),
  saveFirstRunNoticeAcknowledged: vi.fn(() => Promise.resolve()),
}));

function renderList() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerList
        onStartConnect={vi.fn()}
        connectingServerIds={new Set()}
        findExistingSessionId={() => undefined}
        onSwitchToExistingTab={vi.fn()}
        collapsedGroupIds={new Set()}
        onToggleGroup={vi.fn()}
        onCreateFirstServer={vi.fn()}
        onCreateServerInGroup={vi.fn()}
      />
    </I18nextProvider>,
  );
}

describe("copy marker in the server list", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(listServers).mockResolvedValue([
      server("a", "imported-srv"),
      server("b", "old-srv", { isCopy: true, groupId: "g-old" }),
    ]);
    vi.mocked(listGroups).mockResolvedValue([grp("g-old", "Previous data", false), grp("g-copy", "old-group", true)]);
  });

  it("shows the badge only on copies, with an explaining tooltip", async () => {
    renderList();
    await screen.findByText("old-srv");
    const badges = screen.getAllByTestId("copy-badge");
    expect(badges).toHaveLength(2); // the copy server and the copy group
    expect(badges[0]).toHaveAttribute("title", testI18n.t("copyMarker.badgeHint"));
    const importedRow = screen.getByText("imported-srv").closest("button");
    expect(within(importedRow as HTMLElement).queryByTestId("copy-badge")).toBeNull();
  });

  it("clears the marker of a server from its context menu", async () => {
    renderList();
    const row = (await screen.findByText("old-srv")).closest("button") as HTMLElement;
    fireEvent.contextMenu(row);
    vi.mocked(listServers).mockResolvedValue([server("b", "old-srv", { groupId: "g-old" })]);
    fireEvent.click(await screen.findByRole("button", { name: KEEP }));
    await waitFor(() => expect(keepServerAsRegularEntry).toHaveBeenCalledWith("b"));
    await waitFor(() => expect(screen.queryAllByTestId("copy-badge")).toHaveLength(1));
  });

  it("clears a group only after the confirmation, which names the counts", async () => {
    renderList();
    const header = (await screen.findByText(/old-group/)).closest("button") as HTMLElement;
    fireEvent.contextMenu(header);
    fireEvent.click(await screen.findByRole("button", { name: KEEP }));
    const dialog = await screen.findByRole("dialog");
    expect(keepGroupAsRegularEntry).toHaveBeenCalledWith("g-copy", false);
    expect(within(dialog).getByText(testI18n.t("copyMarker.groupConfirmBody", { groups: 2, servers: 3 }))).toBeInTheDocument();
    expect(keepGroupAsRegularEntry).not.toHaveBeenCalledWith("g-copy", true);
    fireEvent.click(within(dialog).getByRole("button", { name: KEEP }));
    await waitFor(() => expect(keepGroupAsRegularEntry).toHaveBeenCalledWith("g-copy", true));
  });

  it("offers no menu entry on a regular group", async () => {
    renderList();
    const header = (await screen.findByText(/Previous data/)).closest("button") as HTMLElement;
    fireEvent.contextMenu(header);
    expect(screen.queryByRole("button", { name: KEEP })).toBeNull();
  });
});
