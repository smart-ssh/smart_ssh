// Issue #12 / ADR 0104: die Host-Key-Abfrage (`host-key-verification-needed`)
// muss in jedem Tab-Zustand erscheinen — auch wenn der "Verwalten"- oder
// "Filter-Regeln"-Tab aktiv ist (dort ist `ServerList` nicht gemountet) und
// wenn ein Session-Tab aktiv ist (der ganze `MainScreen` ist dann
// ausgeblendet). Typischer Auslöser: ein vom Backend gestarteter
// Verbindungsaufbau, z. B. über MCP.
//
// Schwere Kind-Ansichten (`ManagementView`, `FilterRulesView`,
// `SessionView`, …) sind hier durch Platzhalter ersetzt; `ServerList`,
// `HostKeyDialog`, `ToastHost` und `useSessionTabs` laufen echt.
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { confirmHostKey, connect, listServers, listSessions } from "./api";
import { onHostKeyVerificationEnded, onHostKeyVerificationNeeded } from "./events";
import { testI18n } from "./testI18n";
import type {
  HostKeyVerificationEndedEvent,
  HostKeyVerificationNeededEvent,
  ServerDto,
} from "./types";

vi.mock("./api", async () => {
  const actual = await vi.importActual<typeof import("./api")>("./api");
  return {
    commandErrorMessage: actual.commandErrorMessage,
    commandErrorCode: actual.commandErrorCode,
    listAiProviders: vi.fn(() => Promise.resolve([])),
    listServers: vi.fn(() => Promise.resolve([])),
    listGroups: vi.fn(() => Promise.resolve([])),
    listChatSessions: vi.fn(() => Promise.resolve([])),
    listSessions: vi.fn(() => Promise.resolve([])),
    connect: vi.fn(),
    confirmHostKey: vi.fn(),
    resumeChatSession: vi.fn(),
    disconnect: vi.fn(),
    getServer: vi.fn(),
    respondToAction: vi.fn(),
  };
});

// Jeder Event-Listener ist ein No-op — außer `host-key-verification-needed`,
// dessen Abonnenten hier mitgezählt werden (genau einer darf existieren).
vi.mock("./events", async () => {
  const actual = await vi.importActual<Record<string, unknown>>("./events");
  return Object.fromEntries(
    Object.keys(actual).map((name) => [name, vi.fn(() => Promise.resolve(() => {}))]),
  );
});

vi.mock("./firstRunNotice", () => ({
  loadFirstRunNoticeAcknowledged: vi.fn(() => Promise.resolve(true)),
  saveFirstRunNoticeAcknowledged: vi.fn(() => Promise.resolve()),
}));

vi.mock("./components/AppHeader", () => ({
  AppHeader: ({ children }: { children: React.ReactNode }) => <header>{children}</header>,
}));
vi.mock("./components/ManagementView", () => ({
  ManagementView: () => (
    <div data-testid="management-view">
      <input aria-label="server name" />
    </div>
  ),
}));
vi.mock("./components/FilterRulesView", () => ({
  FilterRulesView: () => (
    <div data-testid="filter-rules-view">
      <input aria-label="rule pattern" />
    </div>
  ),
}));
vi.mock("./components/SessionView", () => ({
  SessionView: ({ sessionId }: { sessionId: string }) => (
    <div data-testid={`session-${sessionId}`}>
      <input aria-label="terminal" />
    </div>
  ),
}));
vi.mock("./components/SettingsScreen", () => ({ SettingsScreen: () => null }));
vi.mock("./components/NoteSuggestionToast", () => ({ NoteSuggestionToast: () => null }));
vi.mock("./components/NoteShrinkSuggestionToast", () => ({
  NoteShrinkSuggestionToast: () => null,
}));

const sessionId = "33333333-3333-4333-8333-333333333333";
const events: HostKeyVerificationNeededEvent[] = [
  {
    sessionId,
    promptId: 1,
    host: "prod-1.internal",
    port: 2222,
    kind: "unknown",
    fingerprint: "SHA256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    expectedFingerprint: null,
  },
  {
    sessionId,
    promptId: 2,
    host: "prod-1.internal",
    port: 2222,
    kind: "mismatch",
    fingerprint: "SHA256:newnewnewnewnewnewnewnewnewnewnewnewnewn",
    expectedFingerprint: "SHA256:oldoldoldoldoldoldoldoldoldoldoldoldold",
  },
];

const roleFor = (event: HostKeyVerificationNeededEvent) =>
  event.kind === "mismatch" ? "alertdialog" : "dialog";
const rejectLabelFor = (event: HostKeyVerificationNeededEvent) =>
  event.kind === "mismatch" ? "Verbindung abbrechen" : "Ablehnen";

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

let subscribers: Set<(event: HostKeyVerificationNeededEvent) => void>;
let endSubscribers: Set<(event: HostKeyVerificationEndedEvent) => void>;

function emitHostKeyEnded(event: HostKeyVerificationEndedEvent) {
  act(() => {
    for (const handler of endSubscribers) handler(event);
  });
}

function emitHostKey(event: HostKeyVerificationNeededEvent) {
  act(() => {
    for (const handler of subscribers) handler(event);
  });
}

// `jsdom` kennt Tailwind nicht — ohne diese Regel wäre der per `hidden`
// ausgeblendete `MainScreen`-Zweig für `toBeVisible()` weiterhin sichtbar.
beforeAll(() => {
  const style = document.createElement("style");
  style.textContent = ".hidden { display: none; }";
  document.head.appendChild(style);
});

beforeEach(() => {
  subscribers = new Set();
  vi.mocked(onHostKeyVerificationNeeded).mockImplementation((handler) => {
    subscribers.add(handler);
    return Promise.resolve(() => {
      subscribers.delete(handler);
    });
  });
  endSubscribers = new Set();
  vi.mocked(onHostKeyVerificationEnded).mockImplementation((handler) => {
    endSubscribers.add(handler);
    return Promise.resolve(() => {
      endSubscribers.delete(handler);
    });
  });
  vi.mocked(confirmHostKey).mockResolvedValue(undefined);
});

afterEach(() => {
  vi.mocked(listServers).mockReset();
  vi.mocked(listServers).mockResolvedValue([]);
  vi.mocked(listSessions).mockReset();
  vi.mocked(listSessions).mockResolvedValue([]);
  vi.mocked(confirmHostKey).mockReset();
  vi.mocked(connect).mockReset();
});

let appRoot: HTMLElement;

function renderApp() {
  const result = render(
    <I18nextProvider i18n={testI18n}>
      <App />
    </I18nextProvider>,
  );
  appRoot = result.container;
  return result;
}

async function expectVisibleFocusedDialog(
  event: HostKeyVerificationNeededEvent,
  hiddenSubtree: HTMLElement | null,
) {
  const dialog = await screen.findByRole(roleFor(event));
  expect(screen.getAllByRole(roleFor(event))).toHaveLength(1);
  // Portal: unter `document.body`, aber außerhalb des gerenderten App-Baums.
  expect(document.body).toContainElement(dialog);
  expect(appRoot).not.toContainElement(dialog);
  if (hiddenSubtree) expect(hiddenSubtree).not.toContainElement(dialog);
  expect(dialog).toBeVisible();
  const reject = within(dialog).getByRole("button", { name: rejectLabelFor(event) });
  expect(document.activeElement).toBe(reject);
  return { dialog, reject };
}

describe("App host key prompt in every tab state (Issue #12)", () => {
  it.each([
    { tabLabel: "Verwalten", viewTestId: "management-view", inputLabel: "server name" },
    { tabLabel: "Filter-Regeln", viewTestId: "filter-rules-view", inputLabel: "rule pattern" },
  ])("shows the dialogs while the $tabLabel tab is active", async ({ tabLabel, viewTestId, inputLabel }) => {
    renderApp();
    fireEvent.click(screen.getByRole("button", { name: tabLabel }));
    const view = await screen.findByTestId(viewTestId);
    await waitFor(() => expect(subscribers.size).toBe(1));

    for (const event of events) {
      within(view).getByLabelText(inputLabel).focus();
      emitHostKey(event);
      const { reject } = await expectVisibleFocusedDialog(event, view);

      fireEvent.click(reject);
      await waitFor(() =>
        expect(confirmHostKey).toHaveBeenCalledWith(event.sessionId, { decision: "reject" }),
      );
      expect(screen.queryByRole(roleFor(event))).toBeNull();
      vi.mocked(confirmHostKey).mockClear();
    }
  });

  it("shows the dialogs while a session tab is active and the main screen is hidden", async () => {
    vi.mocked(listSessions).mockResolvedValue([
      {
        sessionId: "44444444-4444-4444-8444-444444444444",
        serverId: "remote-1",
        serverName: "prod-1",
        status: "connected",
        hasPendingAction: false,
      },
    ] as Awaited<ReturnType<typeof listSessions>>);
    renderApp();
    const sessionTab = await screen.findByTestId("session-44444444-4444-4444-8444-444444444444");
    await waitFor(() => expect(sessionTab).toBeVisible());
    const mainScreenBranch = screen
      .getByRole("button", { name: "Verbinden", hidden: true })
      .closest(".hidden");
    expect(mainScreenBranch).not.toBeNull();
    expect(mainScreenBranch).not.toBeVisible();
    await waitFor(() => expect(subscribers.size).toBe(1));

    for (const event of events) {
      within(sessionTab).getByLabelText("terminal").focus();
      emitHostKey(event);
      const { dialog, reject } = await expectVisibleFocusedDialog(
        event,
        mainScreenBranch as HTMLElement,
      );
      expect(sessionTab).not.toContainElement(dialog);

      fireEvent.click(reject);
      await waitFor(() =>
        expect(confirmHostKey).toHaveBeenCalledWith(event.sessionId, { decision: "reject" }),
      );
      vi.mocked(confirmHostKey).mockClear();
    }
  });

  it("renders exactly one dialog while the Connect tab (with ServerList) is active", async () => {
    vi.mocked(listServers).mockResolvedValue([remoteServer()]);
    renderApp();
    await screen.findByText("prod-1");
    await waitFor(() => expect(subscribers.size).toBe(1));

    for (const event of events) {
      emitHostKey(event);
      await expectVisibleFocusedDialog(event, null);
      expect(screen.getAllByRole(roleFor(event))).toHaveLength(1);
      fireEvent.click(screen.getByRole("button", { name: rejectLabelFor(event) }));
      await waitFor(() => expect(screen.queryByRole(roleFor(event))).toBeNull());
    }
    // Ein Tab-Wechsel darf keinen zweiten Abonnenten hinzufügen oder den
    // einzigen entfernen.
    fireEvent.click(screen.getByRole("button", { name: "Verwalten" }));
    await screen.findByTestId("management-view");
    fireEvent.click(screen.getByRole("button", { name: "Verbinden" }));
    await screen.findByText("prod-1");
    expect(subscribers.size).toBe(1);
  });

  it("shows an error toast when confirmHostKey rejects (e.g. after the backend timed out)", async () => {
    vi.mocked(confirmHostKey).mockRejectedValue({
      code: null,
      message: "keine ausstehende Host-Key-Bestätigung",
    });
    renderApp();
    fireEvent.click(screen.getByRole("button", { name: "Verwalten" }));
    await screen.findByTestId("management-view");
    await waitFor(() => expect(subscribers.size).toBe(1));

    emitHostKey(events[0]);
    fireEvent.click(await screen.findByRole("button", { name: "Vertrauen" }));

    await waitFor(() =>
      expect(confirmHostKey).toHaveBeenCalledWith(sessionId, { decision: "trust" }),
    );
    const toast = await screen.findByText(/keine ausstehende Host-Key-Bestätigung/);
    expect(toast).toBeVisible();
    expect(screen.getByRole("status")).toContainElement(toast);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("clears the prompt when the ServerList connect that requested it settles", async () => {
    vi.mocked(listServers).mockResolvedValue([remoteServer()]);
    let rejectConnect: ((err: unknown) => void) | null = null;
    vi.mocked(connect).mockImplementation(
      () =>
        new Promise<string>((_, reject) => {
          rejectConnect = reject;
        }),
    );
    renderApp();
    fireEvent.click(await screen.findByText("prod-1"));
    await waitFor(() => expect(connect).toHaveBeenCalledWith("remote-1"));

    emitHostKey(events[1]);
    await screen.findByRole("alertdialog");

    await act(async () => {
      rejectConnect?.({ code: null, message: "Verbindung fehlgeschlagen" });
    });

    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(confirmHostKey).not.toHaveBeenCalled();
  });
});

// Issue #37: das Backend meldet über `host-key-verification-ended`, dass es
// nicht mehr auf eine Abfrage wartet (Timeout, Abbruch). Die angezeigte
// Abfrage schließt nur, wenn `sessionId` und `promptId` passen.
describe("App host key prompt closes when the backend ends it (Issue #37)", () => {
  async function renderOnManagementTab() {
    renderApp();
    fireEvent.click(screen.getByRole("button", { name: "Verwalten" }));
    await screen.findByTestId("management-view");
    await waitFor(() => expect(subscribers.size).toBe(1));
    await waitFor(() => expect(endSubscribers.size).toBe(1));
  }

  it("closes the shown prompt when a matching end event arrives", async () => {
    await renderOnManagementTab();
    emitHostKey(events[0]);
    await screen.findByRole("dialog");

    emitHostKeyEnded({ sessionId, promptId: events[0].promptId, reason: "abandoned" });

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(confirmHostKey).not.toHaveBeenCalled();
  });

  it("shows an info toast, no error toast and never calls confirmHostKey after a timeout", async () => {
    await renderOnManagementTab();
    emitHostKey(events[1]);
    await screen.findByRole("alertdialog");

    emitHostKeyEnded({ sessionId, promptId: events[1].promptId, reason: "timed_out" });

    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    const toast = await screen.findByText(/Host-Key-Abfrage für prod-1\.internal:2222 ist abgelaufen/);
    expect(toast).toBeVisible();
    expect(toast.textContent).toMatch(/^ℹ /);
    const region = screen.getByRole("status");
    expect(region).toContainElement(toast);
    expect(region.textContent).not.toContain("⚠");
    expect(region.querySelector(".bg-red-950")).toBeNull();
    expect(confirmHostKey).not.toHaveBeenCalled();
  });

  it("keeps a newer prompt open for an end event of another session or an older prompt", async () => {
    await renderOnManagementTab();
    emitHostKey(events[1]);
    await screen.findByRole("alertdialog");

    // Andere Session, gleiche promptId.
    emitHostKeyEnded({
      sessionId: "55555555-5555-4555-8555-555555555555",
      promptId: events[1].promptId,
      reason: "timed_out",
    });
    // Ältere Abfrage derselben Session (z. B. vor einem Retry).
    emitHostKeyEnded({ sessionId, promptId: events[0].promptId, reason: "abandoned" });
    emitHostKeyEnded({ sessionId, promptId: events[0].promptId, reason: "timed_out" });

    expect(screen.getByRole("alertdialog")).toBeVisible();
    expect(screen.queryByRole("status")).toBeNull();
    expect(confirmHostKey).not.toHaveBeenCalled();

    // Die Abfrage bleibt bedienbar.
    fireEvent.click(screen.getByRole("button", { name: rejectLabelFor(events[1]) }));
    await waitFor(() =>
      expect(confirmHostKey).toHaveBeenCalledWith(sessionId, { decision: "reject" }),
    );
  });
});
