// Issue #548: the connecting tab — opens before `connect` resolves, follows
// `connect-progress`, turns into the session tab in place, stays on failure,
// retries in place and cancels on close.
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { cancelConnect, connect, disconnect } from "./api";
import { onConnectProgress } from "./events";
import "./testI18n";
import type { ConnectProgressEvent } from "./types";
import { useSessionTabs } from "./useSessionTabs";

vi.mock("./api", async () => {
  const actual = await vi.importActual<typeof import("./api")>("./api");
  return {
    commandErrorCode: actual.commandErrorCode,
    commandErrorConnectLog: actual.commandErrorConnectLog,
    commandErrorMessage: actual.commandErrorMessage,
    connect: vi.fn(),
    cancelConnect: vi.fn(() => Promise.resolve()),
    disconnect: vi.fn(() => Promise.resolve()),
    getServer: vi.fn(),
    listSessions: vi.fn(() => Promise.resolve([])),
    respondToAction: vi.fn(() => Promise.resolve()),
  };
});

vi.mock("./events", () => ({
  onActionDecisionEscalated: vi.fn(() => Promise.resolve(() => {})),
  onChatActionProposed: vi.fn(() => Promise.resolve(() => {})),
  onChatActionResult: vi.fn(() => Promise.resolve(() => {})),
  onConnectProgress: vi.fn(() => Promise.resolve(() => {})),
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onMcpActionTabRequested: vi.fn(() => Promise.resolve(() => {})),
}));

const server = { id: "srv-1", name: "prod-db" };

type Deferred = { resolve: (id: string) => void; reject: (e: unknown) => void };
function deferConnect(): Deferred {
  const d = {} as Deferred;
  vi.mocked(connect).mockImplementationOnce(
    () =>
      new Promise<string>((resolve, reject) => {
        d.resolve = resolve;
        d.reject = reject;
      }),
  );
  return d;
}

function emitProgress(event: ConnectProgressEvent) {
  const handler = vi.mocked(onConnectProgress).mock.calls[0][0];
  act(() => handler(event));
}

const step = (
  kind: ConnectProgressEvent["steps"][number]["kind"],
  status: "running" | "ok" | "failed",
) => ({ kind, hopIndex: 0, hop: "u@h:22", status, errorCode: null, detail: null });

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(connect).mockReset();
});

describe("connecting tab (issue #548)", () => {
  it("opens and focuses a connecting tab before connect resolves", () => {
    deferConnect();
    const { result } = renderHook(() => useSessionTabs());
    act(() => result.current.startConnect(server));
    expect(result.current.tabs).toHaveLength(1);
    expect(result.current.tabs[0]).toMatchObject({ phase: "connecting", serverName: "prod-db" });
    expect(result.current.activeSessionId).toBe(result.current.tabs[0].sessionId);
    expect(connect).toHaveBeenCalledWith("srv-1", result.current.tabs[0].attemptId);
    expect(result.current.findExistingSessionId("srv-1")).toBe(result.current.tabs[0].sessionId);
  });

  it("applies connect-progress events of its attempt only", () => {
    deferConnect();
    const { result } = renderHook(() => useSessionTabs());
    act(() => result.current.startConnect(server));
    const attemptId = result.current.tabs[0].attemptId!;
    emitProgress({ attemptId: "other", serverId: "srv-1", attempt: 1, steps: [step("dnsResolution", "ok")] });
    expect(result.current.tabs[0].progress).toEqual([]);
    emitProgress({ attemptId, serverId: "srv-1", attempt: 1, steps: [step("dnsResolution", "ok")] });
    expect(result.current.tabs[0].progress).toHaveLength(1);
  });

  it("turns into the session tab in place on success", async () => {
    const d = deferConnect();
    const { result } = renderHook(() => useSessionTabs());
    act(() => result.current.openTab("s-0", "srv-0", "other"));
    act(() => result.current.startConnect(server));
    act(() => result.current.openTab("s-2", "srv-2", "third"));
    act(() => result.current.switchTo(result.current.tabs[1].sessionId));
    await act(async () => d.resolve("session-1"));
    expect(result.current.tabs.map((t) => t.sessionId)).toEqual(["s-0", "session-1", "s-2"]);
    expect(result.current.tabs[1]).toMatchObject({ phase: "connected", attemptId: null });
    expect(result.current.activeSessionId).toBe("session-1");
  });

  it("keeps the tab with the translated error on failure, and retries in place", async () => {
    const d = deferConnect();
    const { result } = renderHook(() => useSessionTabs());
    act(() => result.current.startConnect(server));
    const key = result.current.tabs[0].sessionId;
    const firstAttempt = result.current.tabs[0].attemptId;
    emitProgress({ attemptId: firstAttempt!, serverId: "srv-1", attempt: 1, steps: [step("authentication", "failed")] });
    await act(async () => d.reject({ code: "SSH_AUTH_FAILED", message: "raw" }));
    expect(result.current.tabs).toHaveLength(1);
    expect(result.current.tabs[0]).toMatchObject({ phase: "failed", sessionId: key });
    expect(result.current.tabs[0].failure?.message).not.toBe("raw");
    expect(result.current.tabs[0].failure?.message).toBeTruthy();

    const d2 = deferConnect();
    act(() => result.current.retryConnect(key));
    expect(result.current.tabs).toHaveLength(1);
    expect(result.current.tabs[0]).toMatchObject({ phase: "connecting", progress: [], failure: null });
    expect(result.current.tabs[0].attemptId).not.toBe(firstAttempt);
    await act(async () => d2.resolve("session-2"));
    expect(result.current.tabs[0]).toMatchObject({ sessionId: "session-2", phase: "connected" });
  });

  it("cancels the attempt on close, shows no error and does not leak a late session", async () => {
    const d = deferConnect();
    const { result } = renderHook(() => useSessionTabs());
    act(() => result.current.startConnect(server));
    const { sessionId: key, attemptId } = result.current.tabs[0];
    await act(async () => {
      await result.current.requestCloseTab(key);
    });
    expect(cancelConnect).toHaveBeenCalledWith(attemptId);
    expect(result.current.tabs).toHaveLength(0);
    expect(result.current.activeSessionId).toBeNull();
    await act(async () => d.reject({ code: "SSH_CONNECT_CANCELLED", message: "cancelled" }));
    expect(result.current.tabs).toHaveLength(0);
    expect(disconnect).not.toHaveBeenCalled();
  });

  it("disconnects a session that finished after its tab was closed", async () => {
    const d = deferConnect();
    const { result } = renderHook(() => useSessionTabs());
    act(() => result.current.startConnect(server));
    await act(async () => {
      await result.current.requestCloseTab(result.current.tabs[0].sessionId);
    });
    await act(async () => d.resolve("late-session"));
    expect(disconnect).toHaveBeenCalledWith("late-session");
    expect(result.current.tabs).toHaveLength(0);
  });

  it("closing a failed tab only removes it", async () => {
    const d = deferConnect();
    const { result } = renderHook(() => useSessionTabs());
    act(() => result.current.startConnect(server));
    await act(async () => d.reject({ code: "SSH_AUTH_FAILED", message: "x" }));
    await act(async () => {
      await result.current.requestCloseTab(result.current.tabs[0].sessionId);
    });
    expect(result.current.tabs).toHaveLength(0);
    expect(cancelConnect).not.toHaveBeenCalled();
  });
});
