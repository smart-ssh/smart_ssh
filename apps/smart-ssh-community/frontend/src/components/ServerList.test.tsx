// Spec 0046, Fund 5: der lokale Pseudo-Server hat konzeptionell keinen
// Port (direkte Prozessausführung, kein SSH/TCP) — die Backend-DTO liefert
// dafür immer `port: 0` (s. `local_server::synthetic_server`), das darf im
// UI nicht als echter Port `0` erscheinen. Ein normaler Server zeigt
// seinen Port unverändert.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { ServerList } from "./ServerList";
import {
  connect,
  listGroups,
  listServers,
  listUnusableServers,
  moveGroup,
  moveServerToGroup,
} from "../api";
import { subscribeHostKeyPromptClear } from "../hostKeyPromptBus";
import { onHostKeyVerificationNeeded } from "../events";
import { loadFirstRunNoticeAcknowledged, saveFirstRunNoticeAcknowledged } from "../firstRunNotice";
import {
  registerFirstRunNoticeExtension,
  resetRegistryForTests,
  type FirstRunNoticeExtensionContext,
} from "../extensions/registry";

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
    connect: vi.fn(),
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

function renderList(
  onCreateFirstServer: () => void = vi.fn(),
  onCreateServerInGroup: (groupId: string) => void = vi.fn(),
  onToggleGroup: (groupId: string) => void = vi.fn(),
) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerList
        onConnected={vi.fn()}
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

// Issue #51: ein gescheiterter Verbindungsaufbau zeigt unter der Meldung
// das zugeklappte Schritt-Protokoll mit dem markierten Schritt.
describe("ServerList connect step log (issue #51)", () => {
  it("shows collapsed details with the failing step under a failed connect", async () => {
    vi.mocked(connect).mockRejectedValue({
      message: "Verbindung abgelehnt: Connection refused",
      code: "SSH_CONNECTION_REFUSED",
      connect_log: [
        {
          hopIndex: 0,
          hop: "deploy@prod-1.example:22",
          step: { kind: "dnsResolution", host: "prod-1.example", port: 22, addresses: ["192.0.2.10"] },
          status: { state: "ok" },
          durationMs: 4,
        },
        {
          hopIndex: 0,
          hop: "deploy@prod-1.example:22",
          step: { kind: "tcpConnect", address: null, port: 22 },
          status: { state: "failed", code: "SSH_CONNECTION_REFUSED" },
          durationMs: 1,
        },
      ],
    });

    renderList();
    await waitFor(() => expect(screen.getByText("prod-1")).toBeInTheDocument());
    fireEvent.click(screen.getByText("prod-1"));

    const details = await screen.findByTestId("connect-step-log");
    expect(details).not.toHaveAttribute("open");
    const failed = details.querySelectorAll("[data-failed='true']");
    expect(failed).toHaveLength(1);
    expect(failed[0].textContent).toContain("TCP-Verbindung");
    expect(failed[0].textContent).toContain("SSH_CONNECTION_REFUSED");
  });

  it("shows no details when the error carries no step log", async () => {
    vi.mocked(connect).mockRejectedValue({ message: "x", code: "SSH_CONNECTION_FAILED" });
    renderList();
    await waitFor(() => expect(screen.getByText("prod-1")).toBeInTheDocument());
    fireEvent.click(screen.getByText("prod-1"));
    await waitFor(() =>
      expect(screen.getByText(/Verbindung fehlgeschlagen|Connection failed/)).toBeInTheDocument(),
    );
    expect(screen.queryByTestId("connect-step-log")).toBeNull();
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
    // Issue #49: `/📁 Prod/` statt `/Prod/` — die Gruppenzeile hat jetzt
    // zusätzlich einen „+"-Button, dessen Name den Gruppennamen enthält.
    expect(screen.getByRole("button", { name: /📁 Prod/ })).toBeInTheDocument();
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

describe("ServerList new server in a group (issue #49)", () => {
  afterEach(() => {
    vi.mocked(listGroups).mockImplementation(() => Promise.resolve([]));
  });

  it("the + action on a group row opens the form for that group without toggling it", async () => {
    vi.mocked(listGroups).mockImplementation(() =>
      Promise.resolve([group(), group({ id: "group-2", name: "Web", parentId: "group-1" })]),
    );
    vi.mocked(connect).mockClear();
    const onCreateServerInGroup = vi.fn();
    const onToggleGroup = vi.fn();
    renderList(vi.fn(), onCreateServerInGroup, onToggleGroup);

    fireEvent.click(await screen.findByRole("button", {
        name: testI18n.t("mainScreen.newServerInGroup", { name: "Web" }),
      }));

    expect(onCreateServerInGroup).toHaveBeenCalledWith("group-2");
    expect(onToggleGroup).not.toHaveBeenCalled();
    expect(connect).not.toHaveBeenCalled();
  });
});

// Issue #111 / Spec 0031: the first-run notice through the real `ServerList`
// (the component alone is covered in `FirstRunNoticeScreen.test.tsx`). The
// module mock above keeps "acknowledged" for every other test; this block
// switches it to "not acknowledged" for its own tests only.
describe("ServerList first-run notice (issue #111 / Spec 0031)", () => {
  beforeEach(() => {
    vi.mocked(loadFirstRunNoticeAcknowledged).mockResolvedValue(false);
    vi.mocked(saveFirstRunNoticeAcknowledged).mockReset();
    vi.mocked(saveFirstRunNoticeAcknowledged).mockResolvedValue(undefined);
    vi.mocked(connect).mockReset();
    vi.mocked(connect).mockResolvedValue("11111111-1111-4111-8111-111111111111");
  });

  afterEach(() => {
    vi.mocked(loadFirstRunNoticeAcknowledged).mockImplementation(() => Promise.resolve(true));
    vi.mocked(saveFirstRunNoticeAcknowledged).mockImplementation(() => Promise.resolve());
    vi.mocked(connect).mockReset();
    resetRegistryForTests();
    vi.restoreAllMocks();
  });

  const noticeTitle = () => testI18n.t("firstRunNotice.title");

  /** Spec 0031, section 6 (issue #157): registers an extension whose
   * continue handler is `handler`. */
  function registerHandler(id: string, order: number, handler: () => void | Promise<void>) {
    registerFirstRunNoticeExtension({
      id,
      order,
      Component: ({ onContinue }: FirstRunNoticeExtensionContext) => {
        onContinue(handler);
        return <span>{id}</span>;
      },
    });
  }

  /** Renders the list and waits until both the servers and the (not yet
   * given) acknowledgement are loaded, so a click hits the "not
   * acknowledged" branch and not the short "still loading" window. */
  async function renderUnacknowledged() {
    renderList();
    await screen.findByText("prod-1");
    await waitFor(() => expect(loadFirstRunNoticeAcknowledged).toHaveBeenCalled());
    await act(async () => {});
  }

  async function acknowledgeNotice() {
    fireEvent.click(
      screen.getByRole("checkbox", { name: testI18n.t("firstRunNotice.checkboxLabel") }),
    );
    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: testI18n.t("firstRunNotice.continueButton") }),
      );
    });
  }

  it("shows the notice on the first connect and does not call connect while it is open", async () => {
    await renderUnacknowledged();

    fireEvent.click(screen.getByText("prod-1"));

    expect(await screen.findByText(noticeTitle())).toBeInTheDocument();
    await act(async () => {});
    expect(connect).not.toHaveBeenCalled();
    expect(saveFirstRunNoticeAcknowledged).not.toHaveBeenCalled();
  });

  it("after acknowledging, saves once, closes the notice and connects to the clicked server", async () => {
    await renderUnacknowledged();

    fireEvent.click(screen.getByText("prod-1"));
    await screen.findByText(noticeTitle());
    await acknowledgeNotice();

    await waitFor(() => expect(connect).toHaveBeenCalledTimes(1));
    expect(connect).toHaveBeenCalledWith("remote-1");
    expect(saveFirstRunNoticeAcknowledged).toHaveBeenCalledTimes(1);
    expect(screen.queryByText(noticeTitle())).toBeNull();
  });

  it("does not show the notice again on a later connect in the same list", async () => {
    await renderUnacknowledged();

    fireEvent.click(screen.getByText("prod-1"));
    await screen.findByText(noticeTitle());
    await acknowledgeNotice();
    await waitFor(() => expect(connect).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByText("prod-1").closest("button")).toBeEnabled());

    fireEvent.click(screen.getByText("Localhost"));

    await waitFor(() => expect(connect).toHaveBeenCalledTimes(2));
    expect(connect).toHaveBeenLastCalledWith("local");
    expect(screen.queryByText(noticeTitle())).toBeNull();
    expect(saveFirstRunNoticeAcknowledged).toHaveBeenCalledTimes(1);
  });

  it("on a failed save shows the error, does not connect, and shows the notice again next time", async () => {
    vi.mocked(saveFirstRunNoticeAcknowledged).mockRejectedValueOnce({
      code: null,
      message: "settings store write failed",
    });
    await renderUnacknowledged();

    fireEvent.click(screen.getByText("prod-1"));
    await screen.findByText(noticeTitle());
    await acknowledgeNotice();

    expect(await screen.findByText("settings store write failed")).toBeInTheDocument();
    expect(saveFirstRunNoticeAcknowledged).toHaveBeenCalledTimes(1);
    expect(connect).not.toHaveBeenCalled();
    expect(screen.queryByText(noticeTitle())).toBeNull();

    fireEvent.click(screen.getByText("prod-1"));

    expect(await screen.findByText(noticeTitle())).toBeInTheDocument();
    await act(async () => {});
    expect(connect).not.toHaveBeenCalled();
  });

  // Spec 0031, section 6 (issue #157).
  it("calls extension handlers once after the acknowledgement is stored", async () => {
    const order: string[] = [];
    vi.mocked(saveFirstRunNoticeAcknowledged).mockImplementation(async () => {
      order.push("save");
    });
    const handler = vi.fn(() => {
      order.push("handler");
    });
    registerHandler("ext", 1, handler);
    await renderUnacknowledged();

    fireEvent.click(screen.getByText("prod-1"));
    await screen.findByText(noticeTitle());
    expect(screen.getByText("ext")).toBeInTheDocument();
    await acknowledgeNotice();

    await waitFor(() => expect(connect).toHaveBeenCalledTimes(1));
    expect(handler).toHaveBeenCalledTimes(1);
    expect(order).toEqual(["save", "handler"]);
  });

  it("a throwing extension handler does not keep the notice open or block the connect", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    const other = vi.fn();
    registerHandler("broken", 1, () => {
      throw new Error("extension failed");
    });
    registerHandler("other", 2, other);
    await renderUnacknowledged();

    fireEvent.click(screen.getByText("prod-1"));
    await screen.findByText(noticeTitle());
    await acknowledgeNotice();

    await waitFor(() => expect(connect).toHaveBeenCalledWith("remote-1"));
    expect(screen.queryByText(noticeTitle())).toBeNull();
    expect(saveFirstRunNoticeAcknowledged).toHaveBeenCalledTimes(1);
    expect(other).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalled();
  });

  it("does not call extension handlers when storing the acknowledgement fails", async () => {
    vi.mocked(saveFirstRunNoticeAcknowledged).mockRejectedValueOnce({
      code: null,
      message: "settings store write failed",
    });
    const handler = vi.fn();
    registerHandler("ext", 1, handler);
    await renderUnacknowledged();

    fireEvent.click(screen.getByText("prod-1"));
    await screen.findByText(noticeTitle());
    await acknowledgeNotice();

    expect(await screen.findByText("settings store write failed")).toBeInTheDocument();
    expect(handler).not.toHaveBeenCalled();
    expect(connect).not.toHaveBeenCalled();
  });
});

describe("ServerList not-usable servers (issue #100)", () => {
  it("shows a not-usable server with its reason, and clicking it never connects", async () => {
    vi.mocked(listGroups).mockResolvedValueOnce([group()]);
    vi.mocked(listUnusableServers).mockResolvedValueOnce([
      {
        id: "unusable-1",
        name: "newer-box",
        host: "newer.internal",
        groupId: "group-1",
        reason: "unknown_auth_method",
      },
    ]);
    vi.mocked(connect).mockClear();

    renderList();

    const row = await screen.findByTestId("unusable-server-row");
    expect(row).toHaveTextContent("newer-box");
    expect(row).toHaveTextContent("Nicht nutzbar");
    expect(row).toHaveTextContent(
      "Mit einer neueren Version der App gespeichert; diese Version kennt die Anmeldeart nicht.",
    );
    // The readable servers are listed as usual.
    expect(screen.getByText("prod-1")).toBeInTheDocument();
    // No button, so nothing to connect or drag.
    expect(row.querySelector("button")).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByText("newer-box"));
    });
    expect(connect).not.toHaveBeenCalled();
  });
});
