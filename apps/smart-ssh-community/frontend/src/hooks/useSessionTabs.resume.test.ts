// Spec 0114 §4: refresh of the session tabs when the app returns from the background.
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listSessions } from "../api/api";
import "../testI18n";
import { useSessionTabs } from "./useSessionTabs";

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    listSessions: vi.fn(() => Promise.resolve([])),
  }),
);

vi.mock("../api/events", () => ({
  onActionDecisionEscalated: vi.fn(() => Promise.resolve(() => {})),
  onChatActionProposed: vi.fn(() => Promise.resolve(() => {})),
  onChatActionResult: vi.fn(() => Promise.resolve(() => {})),
  onConnectProgress: vi.fn(() => Promise.resolve(() => {})),
  onConnectWarnings: vi.fn(() => Promise.resolve(() => {})),
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onMcpActionTabRequested: vi.fn(() => Promise.resolve(() => {})),
  onChatSessionChanged: vi.fn(() => Promise.resolve(() => {})),
}));

function summary(status: string, hasPendingAction: boolean) {
  return {
    chatSessionId: null,
    sessionId: "session-1",
    serverId: "server-1",
    serverName: "prod-db",
    status,
    hasPendingAction,
    mcp: null,
  } as never;
}

function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { value: state, configurable: true });
}

function fireVisibilityChange() {
  document.dispatchEvent(new Event("visibilitychange"));
}

async function renderWithOneTab() {
  const view = renderHook(() => useSessionTabs());
  await act(async () => {
    await Promise.resolve();
  });
  return view;
}

async function resume() {
  setVisibility("visible");
  await act(async () => {
    fireVisibilityChange();
    await Promise.resolve();
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listSessions).mockResolvedValue([summary("connected", false)]);
});

afterEach(() => {
  setVisibility("visible");
  vi.restoreAllMocks();
});

describe("useSessionTabs / refresh on resume (Spec 0114 §4)", () => {
  it("updates the status of a tab whose backend status changed", async () => {
    const { result } = await renderWithOneTab();
    expect(result.current.tabs[0].status).toBe("connected");

    vi.mocked(listSessions).mockResolvedValue([summary("disconnected", false)]);
    await resume();

    expect(result.current.tabs[0].status).toBe("disconnected");
  });

  it("updates hasPendingAction from the backend summary", async () => {
    const { result } = await renderWithOneTab();
    expect(result.current.tabs[0].hasPendingAction).toBe(false);

    vi.mocked(listSessions).mockResolvedValue([summary("connected", true)]);
    await resume();

    expect(result.current.tabs[0].hasPendingAction).toBe(true);
  });

  it("does not refresh while the document is hidden", async () => {
    const { result } = await renderWithOneTab();
    vi.mocked(listSessions).mockClear();
    vi.mocked(listSessions).mockResolvedValue([summary("disconnected", true)]);

    setVisibility("hidden");
    await act(async () => {
      fireVisibilityChange();
      await Promise.resolve();
    });

    expect(listSessions).not.toHaveBeenCalled();
    expect(result.current.tabs[0].status).toBe("connected");
  });

  it("leaves tabs missing from the backend list unchanged", async () => {
    const { result } = await renderWithOneTab();
    const before = result.current.tabs[0];

    vi.mocked(listSessions).mockResolvedValue([]);
    await resume();

    expect(listSessions).toHaveBeenCalledTimes(2);
    expect(result.current.tabs[0]).toBe(before);
  });

  it("does not throw or change tabs when listSessions rejects", async () => {
    const { result } = await renderWithOneTab();
    const before = result.current.tabs[0];
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    vi.mocked(listSessions).mockRejectedValue(new Error("boom"));
    await resume();

    expect(listSessions).toHaveBeenCalledTimes(2);
    expect(errSpy).toHaveBeenCalled();
    expect(result.current.tabs[0]).toBe(before);
  });

  it("removes the listener on unmount", async () => {
    const { unmount } = await renderWithOneTab();
    unmount();
    vi.mocked(listSessions).mockClear();

    await resume();

    expect(listSessions).not.toHaveBeenCalled();
  });
});
