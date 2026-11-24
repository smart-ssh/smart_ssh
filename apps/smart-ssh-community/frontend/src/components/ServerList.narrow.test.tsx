// Spec 0046, Fund 5: der lokale Pseudo-Server hat konzeptionell keinen
// Port (direkte Prozessausführung, kein SSH/TCP) — die Backend-DTO liefert
// dafür immer `port: 0` (s. `local_server::synthetic_server`), das darf im
// UI nicht als echter Port `0` erscheinen. Ein normaler Server zeigt
// seinen Port unverändert.
import { cleanup, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../api/types";
import { ServerList } from "./ServerList";
import { setNarrow, setWide } from "../test/matchMedia";
import { listGroups, listServers } from "../api/api";

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
    elevationAllowed: true,
    isCopy: false,
    startDirectory: null,
  };
}

function remoteServer(): ServerDto {
  return { isCopy: false, startDirectory: null,
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
    elevationAllowed: true,
  };
}

vi.mock("../api/api", async () => {
  // Spec 0047, Fund D2: `commandErrorCode`/`commandErrorMessage` bleiben die
  // echten Implementierungen (nicht gemockt) statt der bisherigen
  // Ad-hoc-`String(err)`-Attrappe — der neue Test unten prüft genau ihr
  // Zusammenspiel mit `translateErrorCode` in `ServerList`s `describeError`.
  return (await import("../test/apiMock")).createApiMock({
    listServers: vi.fn(() => Promise.resolve([localServer(), remoteServer()])),
    listUnusableServers: vi.fn(() => Promise.resolve([])),
    listGroups: vi.fn(() => Promise.resolve([] as GroupDto[])),
    listChatSessions: vi.fn(() => Promise.resolve([])),
    resumeChatSession: vi.fn(),
  });
});

vi.mock("../api/events", () => ({
  onHostKeyVerificationNeeded: vi.fn(() => Promise.resolve(() => {})),
  onDataChanged: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../lib/firstRunNotice", () => ({
  loadFirstRunNoticeAcknowledged: vi.fn(() => Promise.resolve(true)),
  saveFirstRunNoticeAcknowledged: vi.fn(() => Promise.resolve()),
}));

const startConnectMock = vi.fn();

function renderList(
  onCreateFirstServer: () => void = vi.fn(),
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
      />
    </I18nextProvider>,
  );
}


function folder(id: string, name: string, parentId: string | null): GroupDto {
  return { isCopy: false, id, name, parentId, notes: "" };
}

describe("ServerList Connect tree: chevrons, connector lines, no row actions (Issue #978, #1367)", () => {
  beforeEach(() => {
    vi.mocked(listGroups).mockResolvedValue([folder("g-a", "Alpha", null), folder("g-b", "Beta", "g-a")]);
    vi.mocked(listServers).mockResolvedValue([
      localServer(),
      { ...remoteServer(), id: "s-a", name: "in-alpha", groupId: "g-a" },
      { ...remoteServer(), id: "s-b", name: "in-beta", groupId: "g-b" },
    ]);
  });
  afterEach(() => {
    setWide();
    vi.mocked(listGroups).mockResolvedValue([]);
    vi.mocked(listServers).mockResolvedValue([localServer(), remoteServer()]);
  });

  it.each([
    ["narrow", 20, setNarrow],
    ["wide", 16, setWide],
  ])("%s: children sit in a ruled container at every depth, with aria-hidden ticks", async (_n, chevron, set) => {
    set();
    renderList();
    const alpha = await screen.findByRole("button", { name: /Alpha/ });
    const beta = await screen.findByRole("button", { name: /Beta/ });
    const inAlpha = (await screen.findByText("in-alpha")).closest("li")!;
    const inBeta = (await screen.findByText("in-beta")).closest("li")!;
    expect(alpha.querySelector("svg")).toHaveAttribute("width", String(chevron));
    // depth 1: Beta and in-alpha share Alpha's container; depth 2: in-beta sits in Beta's.
    const alphaBox = alpha.closest("li")!.querySelector(":scope > [data-testid=tree-children]")!;
    const betaBox = beta.closest("li")!.querySelector(":scope > [data-testid=tree-children]")!;
    expect(alphaBox).toHaveClass("border-l");
    expect(alphaBox).toContainElement(beta);
    expect(alphaBox).toContainElement(inAlpha);
    expect(betaBox).toContainElement(inBeta);
    expect(betaBox.parentElement!.closest("[data-testid=tree-children]")).toBe(alphaBox);
    for (const row of [beta.closest("li")!, inAlpha, inBeta]) {
      const tick = row.querySelector(":scope > [data-testid=tree-tick]");
      expect(tick).toHaveAttribute("aria-hidden", "true");
    }
    // Root-level rows (Alpha, the local server) have no tick.
    expect(alpha.closest("li")!.querySelector(":scope > [data-testid=tree-tick]")).toBeNull();
  });

  it("has no ⋯ / Move to… and no + buttons in either layout", async () => {
    for (const set of [setNarrow, setWide]) {
      set();
      renderList();
      await screen.findByText("in-alpha");
      expect(screen.queryAllByRole("button", { name: /verschieben nach|Move .* to|^⋯$|^\+$/ })).toHaveLength(0);
      cleanup();
    }
  });
});
