// Spec 0092, A3.6/U2: der Tab-Zustand (Hinweis-Indikator, "Tab schließen =
// ablehnen") muss auch auf `action-decision-escalated` reagieren, nicht nur
// auf ein `Confirm` aus `chat-action-proposed` — bislang setzte nur Letzteres
// `hasPendingAction`/`pendingActionId` (s. `onChatActionProposed`-Handler in
// `useSessionTabs.ts`). Ohne den neuen Handler bliebe der Tab-Hinweis aus,
// wenn die KI-Zweitmeinung eine ursprünglich automatisch laufende Aktion erst
// NACH dem Vorschlag auf Rot hebt (A3.1) — und "Tab schließen" würde die
// wartende Bestätigung nicht ablehnen.
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { disconnect, listSessions, respondToAction } from "./api";
import {
  onActionDecisionEscalated,
  onChatActionProposed,
  onChatActionResult,
  onConnectionStatusChanged,
  onMcpActionTabRequested,
} from "./events";
import type { ChatActionProposedEvent, McpActionTabRequestedEvent } from "./types";
import { sessionTabLabel, useSessionTabs } from "./useSessionTabs";

vi.mock("./api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  disconnect: vi.fn(() => Promise.resolve()),
  getServer: vi.fn(() => Promise.resolve({ id: "server-1", name: "prod-db" })),
  listSessions: vi.fn(() => Promise.resolve([])),
  respondToAction: vi.fn(() => Promise.resolve()),
}));

vi.mock("./events", () => ({
  onActionDecisionEscalated: vi.fn(() => Promise.resolve(() => {})),
  onChatActionProposed: vi.fn(() => Promise.resolve(() => {})),
  onChatActionResult: vi.fn(() => Promise.resolve(() => {})),
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onMcpActionTabRequested: vi.fn(() => Promise.resolve(() => {})),
}));

// jsdom hat kein `window.confirm` — `requestCloseTab` fragt bei einer
// wartenden Aktion nach (Spec 0017, Abschnitt 6).
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listSessions).mockResolvedValue([
    {
      sessionId: "session-1",
      serverId: "server-1",
      serverName: "prod-db",
      status: "connected",
      hasPendingAction: false,
      mcp: null,
    },
  ]);
  window.confirm = vi.fn(() => true);
});

async function renderWithOneTab() {
  const view = renderHook(() => useSessionTabs());
  await act(async () => {
    await Promise.resolve();
  });
  return view;
}

describe("useSessionTabs / action-decision-escalated (Spec 0092, A3.6)", () => {
  it("marks the tab as having a pending action once the event fires", async () => {
    let handler: ((event: { sessionId: string; actionId: string; reason: string; code: string }) => void) | null =
      null;
    vi.mocked(onActionDecisionEscalated).mockImplementation((h) => {
      handler = h;
      return Promise.resolve(() => {});
    });

    const { result } = await renderWithOneTab();
    expect(result.current.tabs[0].hasPendingAction).toBe(false);

    act(() => {
      handler!({ sessionId: "session-1", actionId: "action-1", reason: "rot", code: "FILTER_RED_RISK_REQUIRES_CONFIRM" });
    });

    expect(result.current.tabs[0].hasPendingAction).toBe(true);
    expect(result.current.tabs[0].pendingActionId).toBe("action-1");
  });

  it("rejects exactly that action when the tab is closed afterwards", async () => {
    let handler: ((event: { sessionId: string; actionId: string; reason: string; code: string }) => void) | null =
      null;
    vi.mocked(onActionDecisionEscalated).mockImplementation((h) => {
      handler = h;
      return Promise.resolve(() => {});
    });

    const { result } = await renderWithOneTab();
    act(() => {
      handler!({ sessionId: "session-1", actionId: "action-1", reason: "rot", code: "FILTER_RED_RISK_REQUIRES_CONFIRM" });
    });

    await act(async () => {
      await result.current.requestCloseTab("session-1");
    });

    expect(respondToAction).toHaveBeenCalledWith("session-1", "action-1", { decision: "deny" });
    expect(disconnect).toHaveBeenCalledWith("session-1");
  });

  it("ignores the event for a different session (strict sessionId match)", async () => {
    let handler: ((event: { sessionId: string; actionId: string; reason: string; code: string }) => void) | null =
      null;
    vi.mocked(onActionDecisionEscalated).mockImplementation((h) => {
      handler = h;
      return Promise.resolve(() => {});
    });

    const { result } = await renderWithOneTab();
    act(() => {
      handler!({
        sessionId: "some-other-session",
        actionId: "action-1",
        reason: "rot",
        code: "FILTER_RED_RISK_REQUIRES_CONFIRM",
      });
    });

    expect(result.current.tabs[0].hasPendingAction).toBe(false);
    expect(result.current.tabs[0].pendingActionId).toBeNull();
  });
});

// Registriert, damit die Importe oben (Lint: no-unused-vars) tatsächlich
// gebraucht werden UND ein Grundgerüst-Listener-Test die Registrierung
// selbst absichert — unabhängig von A3.6 fällt sonst nicht auf, wenn
// `onConnectionStatusChanged`/`onChatActionResult` aus dem Effekt fallen.
describe("useSessionTabs baseline listeners", () => {
  it("registers connection-status and chat-action-result listeners on mount", async () => {
    await renderWithOneTab();
    expect(onConnectionStatusChanged).toHaveBeenCalled();
    expect(onChatActionResult).toHaveBeenCalled();
  });
});

// Spec 0104 / Issue #50: MCP-Arbeit läuft in einem eigenen Tab je Server und
// MCP-Client — der Tab erscheint, ohne den Fokus zu stehlen, signalisiert
// wartende Bestätigungen und ist nie der Tab des Nutzers für den Server.
describe("useSessionTabs / MCP tabs (Spec 0104)", () => {
  function captureMcpHandler() {
    let handler: ((event: McpActionTabRequestedEvent) => void) | null = null;
    vi.mocked(onMcpActionTabRequested).mockImplementation((h) => {
      handler = h;
      return Promise.resolve(() => {});
    });
    return () => handler!;
  }

  it("adds the MCP tab without switching away from the active user tab", async () => {
    const mcpHandler = captureMcpHandler();
    const { result } = await renderWithOneTab();
    expect(result.current.activeSessionId).toBe("session-1");

    act(() => {
      mcpHandler()({ sessionId: "mcp-1", serverId: "server-1", clientName: "Claude Code" });
    });

    expect(result.current.activeSessionId).toBe("session-1");
    expect(result.current.tabs).toHaveLength(2);
    expect(result.current.tabs[1]).toMatchObject({
      sessionId: "mcp-1",
      serverId: "server-1",
      mcp: { clientName: "Claude Code" },
    });
  });

  it("keeps the overview active when an MCP tab appears with no tab open", async () => {
    vi.mocked(listSessions).mockResolvedValue([]);
    const mcpHandler = captureMcpHandler();
    const { result } = await renderWithOneTab();

    act(() => {
      mcpHandler()({ sessionId: "mcp-1", serverId: "server-1", clientName: null });
    });

    expect(result.current.activeSessionId).toBeNull();
    expect(result.current.tabs[0].mcp).toEqual({ clientName: null });
  });

  it("signals a pending confirmation on the background MCP tab", async () => {
    const mcpHandler = captureMcpHandler();
    let proposed: ((event: ChatActionProposedEvent) => void) | null = null;
    vi.mocked(onChatActionProposed).mockImplementation((h) => {
      proposed = h;
      return Promise.resolve(() => {});
    });
    const { result } = await renderWithOneTab();
    act(() => {
      mcpHandler()({ sessionId: "mcp-1", serverId: "server-1", clientName: "Claude Code" });
    });

    act(() => {
      proposed!({
        sessionId: "mcp-1",
        actionId: "action-9",
        decision: { Confirm: { reason: "MCP", code: "FILTER_MCP_ORIGIN_REQUIRES_CONFIRM" } },
      } as unknown as ChatActionProposedEvent);
    });

    const mcpTab = result.current.tabs.find((t) => t.sessionId === "mcp-1")!;
    expect(mcpTab.hasPendingAction).toBe(true);
    expect(result.current.tabs.find((t) => t.sessionId === "session-1")!.hasPendingAction).toBe(false);
    expect(result.current.activeSessionId).toBe("session-1");
  });

  it("never returns an MCP tab as the user's existing tab for a server", async () => {
    vi.mocked(listSessions).mockResolvedValue([]);
    const mcpHandler = captureMcpHandler();
    const { result } = await renderWithOneTab();
    act(() => {
      mcpHandler()({ sessionId: "mcp-1", serverId: "server-1", clientName: "Claude Code" });
    });

    expect(result.current.findExistingSessionId("server-1")).toBeUndefined();

    act(() => {
      result.current.openTab("user-1", "server-1", "prod-db");
    });
    expect(result.current.findExistingSessionId("server-1")).toBe("user-1");
  });

  it("restores MCP tabs after a reload without making them active", async () => {
    vi.mocked(listSessions).mockResolvedValue([
      {
        sessionId: "mcp-1",
        serverId: "server-1",
        serverName: "prod-db",
        status: "connected",
        hasPendingAction: true,
        mcp: { clientName: "Claude Code" },
      },
      {
        sessionId: "session-1",
        serverId: "server-1",
        serverName: "prod-db",
        status: "connected",
        hasPendingAction: false,
        mcp: null,
      },
    ]);
    const { result } = await renderWithOneTab();

    expect(result.current.activeSessionId).toBe("session-1");
    expect(result.current.tabs[0].mcp).toEqual({ clientName: "Claude Code" });
    expect(result.current.tabs[0].hasPendingAction).toBe(true);
  });

  it("labels MCP tabs with client and server", () => {
    expect(sessionTabLabel({ serverName: "web-01", mcp: { clientName: "Claude Code" } }, "Tool")).toBe(
      "Claude Code @ web-01",
    );
    expect(sessionTabLabel({ serverName: "web-01", mcp: { clientName: null } }, "Tool")).toBe("Tool @ web-01");
    expect(sessionTabLabel({ serverName: "web-01", mcp: null }, "Tool")).toBe("web-01");
  });
});
