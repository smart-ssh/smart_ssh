// Spec 0046, Fund 5: der lokale Pseudo-Server hat konzeptionell keinen
// Port (direkte Prozessausführung, kein SSH/TCP) — die Backend-DTO liefert
// dafür immer `port: 0` (s. `local_server::synthetic_server`), das darf im
// UI nicht als echter Port `0` erscheinen. Ein normaler Server zeigt
// seinen Port unverändert.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { ServerList } from "./ServerList";
import { connect, listGroups, listServers, moveGroup, moveServerToGroup } from "../api";
import { subscribeHostKeyPromptClear } from "../hostKeyPromptBus";
import { onHostKeyVerificationNeeded } from "../events";

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
    listGroups: vi.fn(() => Promise.resolve([] as GroupDto[])),
    listChatSessions: vi.fn(() => Promise.resolve([])),
    connect: vi.fn(),
    resumeChatSession: vi.fn(),
    moveServerToGroup: vi.fn(() => Promise.resolve()),
    moveGroup: vi.fn(() => Promise.resolve()),
    commandErrorMessage: actual.commandErrorMessage,
    commandErrorCode: actual.commandErrorCode,
  };
});

vi.mock("../events", () => ({
  onHostKeyVerificationNeeded: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../firstRunNotice", () => ({
  loadFirstRunNoticeAcknowledged: vi.fn(() => Promise.resolve(true)),
  saveFirstRunNoticeAcknowledged: vi.fn(() => Promise.resolve()),
}));

function renderList(onCreateFirstServer: () => void = vi.fn()) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerList
        onConnected={vi.fn()}
        findExistingSessionId={() => undefined}
        onSwitchToExistingTab={vi.fn()}
        collapsedGroupIds={new Set()}
        onToggleGroup={vi.fn()}
        onCreateFirstServer={onCreateFirstServer}
      />
    </I18nextProvider>,
  );
}

function group(overrides: Partial<GroupDto> = {}): GroupDto {
  return {
    id: "group-1",
    name: "Prod",
    parentId: null,
    notes: "",
    ...overrides,
  };
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

describe("ServerList connect-error translation (Spec 0047, Fund D2)", () => {
  afterEach(() => {
    void testI18n.changeLanguage("de");
  });

  it("translates a coded backend connect error instead of showing the raw Display text", async () => {
    // Vor dem Fix hing an `connect_session`s `SshError` kein `code` (der
    // blanket `?` in `commands.rs` verwarf ihn) — das Frontend zeigte den
    // rohen, hart-deutschen `Display`-Text inkl. eingebettetem OS-
    // Fehlertext, UNABHÄNGIG von der UI-Sprache. `lng: "en"` beweist genau
    // das: ohne den Fix stünde hier der deutsche Rohtext in einer
    // englischen UI. Jetzt trägt der Fehler `code: "SSH_CONNECTION_FAILED"`,
    // und `describeError` (s. `ServerList.tsx`) übersetzt darüber — inkl.
    // der von Spec 0047, Fund D2 verlangten Handlungsanleitung ("ist der
    // Host erreichbar?" / "is the host reachable?"), nicht nur einer
    // reinen Zustandsbeschreibung.
    await testI18n.changeLanguage("en");
    vi.mocked(connect).mockRejectedValue({
      message: "Verbindung fehlgeschlagen: Connection refused (os error 61)",
      code: "SSH_CONNECTION_FAILED",
    });

    renderList();
    await waitFor(() => expect(screen.getByText("prod-1")).toBeInTheDocument());

    fireEvent.click(screen.getByText("prod-1"));

    await waitFor(() =>
      expect(
        screen.getByText("Connection failed – is the host reachable (address, port, network)?"),
      ).toBeInTheDocument(),
    );
    expect(screen.queryByText(/os error 61/)).toBeNull();
    expect(screen.queryByText(/Verbindung fehlgeschlagen/)).toBeNull();
  });
});

// Spec 0069, Teil C1 (BL-0082), Tests 27/28.
describe("ServerList empty-state entry block (Spec 0069, Teil C1)", () => {
  it("shows the entry block when only the local pseudo-server exists, and the old dev text is gone", async () => {
    vi.mocked(listServers).mockResolvedValueOnce([localServer()]);
    vi.mocked(listGroups).mockResolvedValueOnce([]);

    renderList();

    await screen.findByText("Noch kein Server angelegt");
    expect(screen.getByRole("button", { name: "Ersten Server anlegen" })).toBeInTheDocument();
    expect(screen.queryByText(/profiles_demo/)).not.toBeInTheDocument();
  });

  it("hides the entry block once a real server exists", async () => {
    vi.mocked(listServers).mockResolvedValueOnce([localServer(), remoteServer()]);
    vi.mocked(listGroups).mockResolvedValueOnce([]);

    renderList();

    await screen.findByText("prod-1");
    expect(screen.queryByText("Noch kein Server angelegt")).not.toBeInTheDocument();
  });

  it("shows both the entry block and the (empty) group tree when only groups exist, no servers", async () => {
    vi.mocked(listServers).mockResolvedValueOnce([localServer()]);
    vi.mocked(listGroups).mockResolvedValueOnce([group()]);

    renderList();

    await screen.findByText("Noch kein Server angelegt");
    expect(screen.getByRole("button", { name: /Prod/ })).toBeInTheDocument();
  });

  it('clicking "Ersten Server anlegen" invokes the callback', async () => {
    vi.mocked(listServers).mockResolvedValueOnce([localServer()]);
    vi.mocked(listGroups).mockResolvedValueOnce([]);
    const onCreateFirstServer = vi.fn();

    renderList(onCreateFirstServer);
    await screen.findByText("Noch kein Server angelegt");

    fireEvent.click(screen.getByRole("button", { name: "Ersten Server anlegen" }));

    expect(onCreateFirstServer).toHaveBeenCalledTimes(1);
  });
});

// Issue #12 / ADR 0104: die Host-Key-Abfrage gehört nicht mehr `ServerList`,
// sondern dem stets gemounteten `HostKeyPromptHost` an der `App`-Wurzel
// (Sichtbarkeit in jedem Tab-Zustand: `App.hostKeyPrompt.test.tsx`).
// `ServerList` darf selbst keinen Listener mehr registrieren (sonst zwei
// Dialoge) und signalisiert nur noch das Ende seines `connect()`.
describe("ServerList host key prompt ownership (Issue #12)", () => {
  it("does not subscribe to host-key-verification-needed itself", async () => {
    renderList();
    await waitFor(() => expect(screen.getByText("prod-1")).toBeInTheDocument());
    expect(onHostKeyVerificationNeeded).not.toHaveBeenCalled();
  });

  it.each([
    ["succeeds", () => Promise.resolve("55555555-5555-4555-8555-555555555555")],
    ["fails", () => Promise.reject({ code: null, message: "kaputt" })],
  ])("clears the host key prompt when its connect() %s", async (_label, outcome) => {
    let finish: (() => void) | null = null;
    vi.mocked(connect).mockImplementation(
      () =>
        new Promise<string>((resolve, reject) => {
          finish = () => outcome().then(resolve, reject);
        }),
    );
    const cleared = vi.fn();
    const unsubscribe = subscribeHostKeyPromptClear(cleared);
    try {
      renderList();
      fireEvent.click(await screen.findByText("prod-1"));
      await waitFor(() => expect(connect).toHaveBeenCalledWith("remote-1"));
      expect(cleared).not.toHaveBeenCalled();

      await act(async () => {
        finish?.();
      });

      await waitFor(() => expect(cleared).toHaveBeenCalledTimes(1));
    } finally {
      unsubscribe();
      vi.mocked(connect).mockReset();
    }
  });
});

describe("ServerList drag and drop (issue #48 / Spec 0103)", () => {
  afterEach(() => {
    // @ts-expect-error -- jsdom hat die Funktion von Haus aus nicht.
    delete document.elementFromPoint;
    vi.mocked(listGroups).mockImplementation(() => Promise.resolve([]));
  });

  function dragOnto(source: HTMLElement, over: HTMLElement | null) {
    document.elementFromPoint = vi.fn(() => over);
    fireEvent.pointerDown(source, { button: 0, buttons: 1, pointerId: 1, clientX: 5, clientY: 5 });
    fireEvent.pointerMove(source, { buttons: 1, pointerId: 1, clientX: 50, clientY: 80 });
    fireEvent.pointerUp(source, { button: 0, buttons: 0, pointerId: 1, clientX: 50, clientY: 80 });
    fireEvent.click(source);
  }

  it("moves a server into a group via the narrow command, reloads, and does not connect", async () => {
    vi.mocked(listGroups).mockImplementation(() => Promise.resolve([group()]));
    renderList();
    await waitFor(() => expect(screen.getByText("prod-1")).toBeInTheDocument());
    const loadsBefore = vi.mocked(listServers).mock.calls.length;

    dragOnto(screen.getByText("prod-1"), screen.getByText(/Prod$/));

    await waitFor(() => expect(moveServerToGroup).toHaveBeenCalledWith("remote-1", "group-1"));
    await waitFor(() =>
      expect(vi.mocked(listServers).mock.calls.length).toBeGreaterThan(loadsBefore),
    );
    expect(connect).not.toHaveBeenCalled();
  });

  it("does not let the local pseudo-server be dragged", async () => {
    vi.mocked(listGroups).mockImplementation(() => Promise.resolve([group()]));
    vi.mocked(connect).mockClear();
    vi.mocked(moveServerToGroup).mockClear();
    renderList();
    await waitFor(() => expect(screen.getByText("Localhost")).toBeInTheDocument());

    dragOnto(screen.getByText("Localhost"), screen.getByText(/Prod$/));

    expect(moveServerToGroup).not.toHaveBeenCalled();
  });

  it("shows the translated cycle error when a group is dropped into its own subgroup", async () => {
    vi.mocked(listGroups).mockImplementation(() =>
      Promise.resolve([group(), group({ id: "group-2", name: "Web", parentId: "group-1" })]),
    );
    vi.mocked(moveGroup).mockImplementationOnce(() =>
      Promise.reject({ message: "Zyklus", code: "GROUP_CYCLE_DETECTED" }),
    );
    renderList();
    await waitFor(() => expect(screen.getByText(/Web$/)).toBeInTheDocument());

    dragOnto(screen.getByText(/Prod$/), screen.getByText(/Web$/));

    await waitFor(() => expect(moveGroup).toHaveBeenCalledWith("group-1", "group-2"));
    expect(
      await screen.findByText(testI18n.t("errors.GROUP_CYCLE_DETECTED")),
    ).toBeInTheDocument();
  });
});
