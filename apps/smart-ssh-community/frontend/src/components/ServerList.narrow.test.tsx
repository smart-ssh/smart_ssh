// Spec 0046, Fund 5: der lokale Pseudo-Server hat konzeptionell keinen
// Port (direkte Prozessausführung, kein SSH/TCP) — die Backend-DTO liefert
// dafür immer `port: 0` (s. `local_server::synthetic_server`), das darf im
// UI nicht als echter Port `0` erscheinen. Ein normaler Server zeigt
// seinen Port unverändert.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { ServerList } from "./ServerList";
import { setNarrow, setWide } from "../test/matchMedia";
import { listGroups, moveGroup, moveServerToGroup } from "../api";

function localServer(): ServerDto {
  return {
    id: "local",
    name: "Localhost",
    host: "localhost",
    port: 0,
    username: "me",
    groupId: null,
    tags: [],
    authKind: "agent",
    identityFilePath: null,
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    sudoPasswordUnknown: false,
    isLocal: true,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
    sftpServerPath: null,
  };
}

function remoteServer(): ServerDto {
  return {
    id: "remote-1",
    name: "prod-1",
    host: "prod-1.internal",
    port: 2222,
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
  };
}

vi.mock("../api", async () => {
  // Spec 0047, Fund D2: `commandErrorCode`/`commandErrorMessage` bleiben die
  // echten Implementierungen (nicht gemockt) statt der bisherigen
  // Ad-hoc-`String(err)`-Attrappe — der neue Test unten prüft genau ihr
  // Zusammenspiel mit `translateErrorCode` in `ServerList`s `describeError`.
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return {
    listServers: vi.fn(() => Promise.resolve([localServer(), remoteServer()])),
    listUnusableServers: vi.fn(() => Promise.resolve([])),
    listGroups: vi.fn(() => Promise.resolve([] as GroupDto[])),
    listChatSessions: vi.fn(() => Promise.resolve([])),
    resumeChatSession: vi.fn(),
    moveServerToGroup: vi.fn(() => Promise.resolve()),
    moveGroup: vi.fn(() => Promise.resolve()),
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

const startConnectMock = vi.fn();

function renderList(
  onCreateFirstServer: () => void = vi.fn(),
  onCreateServerInGroup: (groupId: string) => void = vi.fn(),
  onToggleGroup: (groupId: string) => void = vi.fn(),
) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerList
        onStartConnect={startConnectMock}
        connectingServerIds={new Set()}
        findExistingSessionId={() => undefined}
        onSwitchToExistingTab={vi.fn()}
        collapsedGroupIds={new Set()}
        onToggleGroup={onToggleGroup}
        onCreateFirstServer={onCreateFirstServer}
        onCreateServerInGroup={onCreateServerInGroup}
      />
    </I18nextProvider>,
  );
}


const moveLabel = (name: string) => new RegExp(`^(Move “${name}” to|„${name}“ verschieben)`);

function folder(id: string, name: string, parentId: string | null): GroupDto {
  return { id, name, parentId, notes: "" };
}

describe("ServerList narrow layout: Move to… (Issue #582)", () => {
  beforeEach(() => {
    vi.mocked(moveServerToGroup).mockClear();
    vi.mocked(moveGroup).mockClear();
    vi.mocked(listGroups).mockResolvedValue([folder("g-a", "Alpha", null), folder("g-b", "Beta", "g-a")]);
  });
  afterEach(() => {
    setWide();
    vi.mocked(listGroups).mockResolvedValue([]);
  });

  it("moves a server through the same backend call as drag and drop", async () => {
    setNarrow();
    renderList();
    fireEvent.click(await screen.findByRole("button", { name: moveLabel("prod-1") }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: /^📁 Beta/ }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(moveServerToGroup).toHaveBeenCalledWith("remote-1", "g-b"));
  });

  it("moves a folder to the top level and refuses its own subfolder", async () => {
    setNarrow();
    vi.mocked(listGroups).mockResolvedValue([folder("g-a", "Alpha", null), folder("g-b", "Beta", "g-a")]);
    renderList();
    fireEvent.click(await screen.findByRole("button", { name: moveLabel("Beta") }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("button", { name: /^📁 Beta/ })).toBeDisabled();
    fireEvent.click(within(dialog).getByRole("button", { name: /Ungrouped|Ohne Gruppe/ }));
    await waitFor(() => expect(moveGroup).toHaveBeenCalledWith("g-b", null));
  });

  it("offers no row actions for the local server and none in the wide layout", async () => {
    setNarrow();
    renderList();
    await screen.findByText("prod-1");
    const moveButtons = () => screen.queryAllByRole("button", { name: /verschieben nach|Move .* to/ });
    expect(moveButtons()).toHaveLength(3); // prod-1 and the two folders; the local server has none
    expect(screen.queryByRole("button", { name: moveLabel("Localhost") })).toBeNull();
    setWide();
    await waitFor(() => expect(moveButtons()).toHaveLength(0));
  });
});
