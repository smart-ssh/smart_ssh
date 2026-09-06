// Spec 0046, Fund 5: der lokale Pseudo-Server hat konzeptionell keinen
// Port (direkte Prozessausführung, kein SSH/TCP) — die Backend-DTO liefert
// dafür immer `port: 0` (s. `local_server::synthetic_server`), das darf im
// UI nicht als echter Port `0` erscheinen. Ein normaler Server zeigt
// seinen Port unverändert.
import { render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { ServerList } from "./ServerList";

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
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    isLocal: true,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
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
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    isLocal: false,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
  };
}

vi.mock("../api", () => ({
  listServers: vi.fn(() => Promise.resolve([localServer(), remoteServer()])),
  listGroups: vi.fn(() => Promise.resolve([] as GroupDto[])),
  listChatSessions: vi.fn(() => Promise.resolve([])),
  connect: vi.fn(),
  confirmHostKey: vi.fn(),
  resumeChatSession: vi.fn(),
  commandErrorMessage: (err: unknown) => String(err),
}));

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
        onConnected={vi.fn()}
        findExistingSessionId={() => undefined}
        onSwitchToExistingTab={vi.fn()}
        collapsedGroupIds={new Set()}
        onToggleGroup={vi.fn()}
      />
    </I18nextProvider>,
  );
}

describe("ServerList port display (Spec 0046, Fund 5)", () => {
  it("hides the port for the local pseudo-server", async () => {
    renderList();

    await waitFor(() => expect(screen.getByText("Localhost")).toBeInTheDocument());

    expect(screen.getByText("me@localhost")).toBeInTheDocument();
    expect(screen.queryByText(/:0\b/)).toBeNull();
    expect(screen.queryByText("me@localhost:0")).toBeNull();
  });

  it("still shows the port for a normal remote server", async () => {
    renderList();

    await waitFor(() => expect(screen.getByText("prod-1")).toBeInTheDocument());

    expect(screen.getByText("deploy@prod-1.internal:2222")).toBeInTheDocument();
  });
});
