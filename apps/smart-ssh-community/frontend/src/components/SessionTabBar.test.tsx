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
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { SessionTab } from "../useSessionTabs";
import { SessionTabBar } from "./SessionTabBar";

const tabs: SessionTab[] = [
  {
    sessionId: "s1",
    serverId: "server-1",
    serverName: "prod-db",
    status: "connected",
    hasPendingAction: false,
    pendingActionId: null,
  },
  {
    sessionId: "s2",
    serverId: "server-2",
    serverName: "staging-web",
    status: "disconnected",
    hasPendingAction: false,
    pendingActionId: null,
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
