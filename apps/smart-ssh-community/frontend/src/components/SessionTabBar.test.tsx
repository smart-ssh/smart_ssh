// Regression test for the titlebar-drag fix (Spec 0017/0014): once at
// least one tab is open, dragging the window by the title bar stopped
// working because AppHeader's data-tauri-drag-region attribute does not
// inherit to this component's own DOM subtree (Tauri checks the exact
// clicked element's own ancestor chain, not inherited state — s.
// SessionTabBar.tsx's Doc-Kommentar and the fix commit's message for the
// full mechanism).
//
// What this test CAN verify, on the DOM: the root container and each
// tab's wrapper <div> carry `data-tauri-drag-region`, while every
// interactive <button> inside does NOT carry it itself — that is exactly
// the precondition Tauri's shipped drag-detection algorithm
// (tauri/src/window/scripts/drag.js) depends on to keep clicks on the
// buttons working while making the surrounding tab-bar area draggable.
// What this test CANNOT verify: actual native window-drag behavior itself
// (starting a real OS-level window drag) — that only happens through
// Tauri's own runtime/webview, not through jsdom, so no test here claims
// to cover it.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { SessionTab } from "../useSessionTabs";
import { SessionTabBar } from "./SessionTabBar";

const CONNECTED: Pick<SessionTab, "phase" | "attemptId" | "progress" | "failure"> = {
  phase: "connected",
  attemptId: null,
  progress: [],
  failure: null,
};

const tabs: SessionTab[] = [
  {
    sessionId: "s1",
    serverId: "server-1",
    serverName: "prod-db",
    status: "connected",
    hasPendingAction: false,
    pendingActionId: null,
    mcp: null,
    ...CONNECTED,
  },
  {
    sessionId: "s2",
    serverId: "server-2",
    serverName: "staging-web",
    status: "disconnected",
    hasPendingAction: false,
    pendingActionId: null,
    mcp: null,
    ...CONNECTED,
  },
];

describe("SessionTabBar drag region", () => {
  it("marks the root and each tab wrapper as a drag region, but not the buttons inside", () => {
    const { container } = render(
      <I18nextProvider i18n={testI18n}>
        <SessionTabBar
          tabs={tabs}
          activeSessionId="s1"
          onSwitch={vi.fn()}
          onRequestClose={vi.fn()}
        />
      </I18nextProvider>,
    );

    const root = container.firstElementChild;
    expect(root).toHaveAttribute("data-tauri-drag-region");

    for (const tab of tabs) {
      // Each tab's own wrapper <div> (not the root) carries the attribute
      // — closest() would also match the root itself, so this asserts the
      // wrapper specifically by checking it's a strict descendant of root
      // that isn't root itself.
      const wrapper = screen.getByText(tab.serverName).closest("[data-tauri-drag-region]");
      expect(wrapper).not.toBeNull();
      expect(wrapper).not.toBe(root);
      expect(root).toContainElement(wrapper as HTMLElement);
    }

    for (const button of screen.getAllByRole("button")) {
      expect(button).not.toHaveAttribute("data-tauri-drag-region");
    }
  });
});

// Spec 0104: MCP-Tabs sind als solche erkennbar und signalisieren eine
// wartende Bestätigung aus dem Hintergrund.
describe("SessionTabBar MCP tabs", () => {
  it("labels an MCP tab with client and server, marks it and shows a pending badge", () => {
    const mcpTabs: SessionTab[] = [
      ...tabs,
      {
        sessionId: "s3",
        serverId: "server-1",
        serverName: "prod-db",
        status: "connected",
        hasPendingAction: true,
        pendingActionId: "a1",
        mcp: { clientName: "Claude Code" },
        ...CONNECTED,
      },
    ];
    render(
      <I18nextProvider i18n={testI18n}>
        <SessionTabBar tabs={mcpTabs} activeSessionId="s1" onSwitch={vi.fn()} onRequestClose={vi.fn()} />
      </I18nextProvider>,
    );

    expect(screen.getByText("Claude Code @ prod-db")).toBeInTheDocument();
    expect(screen.getByText("MCP")).toBeInTheDocument();
    expect(screen.getByText(testI18n.t("sessionTabs.mcp.pendingBadge"))).toBeInTheDocument();
    // Der Nutzer-Tab desselben Servers bleibt ein eigener Tab.
    expect(screen.getByText("prod-db")).toBeInTheDocument();
  });
});

describe("SessionTabBar close button frame", () => {
  it("has a fixed 20x20 box with a reserved border and a hover/focus frame", () => {
    render(
      <I18nextProvider i18n={testI18n}>
        <SessionTabBar tabs={tabs} activeSessionId="s1" onSwitch={vi.fn()} onRequestClose={vi.fn()} />
      </I18nextProvider>,
    );
    const close = screen.getByRole("button", { name: testI18n.t("sessionTabs.closeTab", { name: "prod-db" }) });
    // Fixed size + transparent border at rest => no layout shift on hover.
    expect(close).toHaveClass("h-5", "w-5", "border", "border-transparent", "rounded");
    for (const prefix of ["hover:", "focus-visible:"]) {
      expect(close).toHaveClass(`${prefix}border-fg-faint`, `${prefix}bg-border-strong/60`);
    }
  });
});

// Issue #657: context menu "New session" on a tab; numbered labels.
describe("SessionTabBar new session (issue #657)", () => {
  it("offers New session for a user tab and reports its server", () => {
    const onNewSession = vi.fn();
    render(
      <I18nextProvider i18n={testI18n}>
        <SessionTabBar
          tabs={[tabs[0]!, { ...tabs[0]!, sessionId: "s1b", serverNumber: 2 }]}
          activeSessionId="s1"
          onSwitch={vi.fn()}
          onRequestClose={vi.fn()}
          onNewSession={onNewSession}
        />
      </I18nextProvider>,
    );
    expect(screen.getByText("prod-db (2)")).toBeInTheDocument();
    fireEvent.contextMenu(screen.getByText("prod-db (2)"));
    fireEvent.click(screen.getByRole("button", { name: "Neue Sitzung" }));
    expect(onNewSession).toHaveBeenCalledWith({ id: "server-1", name: "prod-db" });
  });
});
