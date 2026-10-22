// Issue #579, Spec 0113 §3: burger menu of the narrow layout.
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { setNarrow, setWide } from "../test/matchMedia";
import { testI18n } from "../testI18n";
import type { SessionTab } from "../useSessionTabs";
import { NarrowHeaderNav } from "./NarrowHeaderNav";

const base = { status: "connected", hasPendingAction: false, pendingActionId: null, mcp: null, attemptId: null, progress: [], failure: null } as const;
const tabs: SessionTab[] = [
  { ...base, sessionId: "s1", serverId: "a", serverName: "prod-db", phase: "connected" },
  { ...base, sessionId: "s2", serverId: "b", serverName: "staging", phase: "connecting" },
];

function setup(activeSessionId: string | null = null) {
  const h = {
    onSelectMainTab: vi.fn(),
    onSwitch: vi.fn(),
    onRequestClose: vi.fn(),
    onOpenSettings: vi.fn(),
  };
  function Host() {
    // Like App: the narrow header exists only while the window is narrow.
    return useIsNarrow() ? (
      <NarrowHeaderNav tabs={tabs} activeSessionId={activeSessionId} tab="manage" {...h} />
    ) : (
      <p>wide header</p>
    );
  }
  render(
    <I18nextProvider i18n={testI18n}>
      <Host />
    </I18nextProvider>,
  );
  return h;
}

const burger = () => screen.getByRole("button", { name: /navigation menu/i });
const open = () => fireEvent.click(burger());

beforeEach(() => testI18n.changeLanguage("en"));
afterEach(async () => {
  setWide();
  await testI18n.changeLanguage("de");
});

describe("NarrowHeaderNav", () => {
  it("shows the burger and the app name on the overview", () => {
    setNarrow();
    setup();
    expect(burger()).toBeInTheDocument();
    expect(screen.getByTestId("narrow-title")).toHaveTextContent("Smart SSH");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows the server name and state of the active session", () => {
    setNarrow();
    setup("s2");
    expect(screen.getByTestId("narrow-title")).toHaveTextContent("staging");
    expect(screen.getByTestId("narrow-title")).toHaveTextContent(/connecting/i);
  });

  it("lists navigation entries and open sessions, marking the current one", () => {
    setNarrow();
    setup();
    open();
    const menu = screen.getByRole("dialog", { name: /navigation menu/i });
    for (const name of ["Overview", "Connect", "Manage", "Filter Rules", "Settings"]) {
      expect(within(menu).getByRole("button", { name })).toBeInTheDocument();
    }
    expect(within(menu).getByRole("button", { name: "Manage" })).toHaveAttribute("aria-current", "page");
    expect(within(menu).getByRole("button", { name: "Connect" })).not.toHaveAttribute("aria-current");
    expect(within(menu).getByRole("button", { name: /^prod-db$/ })).toBeInTheDocument();
    expect(within(menu).getByRole("button", { name: /^Connecting to staging/ })).toBeInTheDocument();
  });

  it("marks the active session instead of the main entries", () => {
    setNarrow();
    setup("s1");
    open();
    expect(screen.getByRole("button", { name: "Manage" })).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("button", { name: /^prod-db$/ })).toHaveAttribute("aria-current", "page");
  });

  it("navigates and closes on selection", () => {
    setNarrow();
    const h = setup();
    open();
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(h.onSelectMainTab).toHaveBeenCalledWith("connect");
    expect(screen.queryByRole("dialog")).toBeNull();

    open();
    fireEvent.click(screen.getByRole("button", { name: "Overview" }));
    expect(h.onSwitch).toHaveBeenCalledWith(null);

    open();
    fireEvent.click(screen.getByRole("button", { name: /^prod-db$/ }));
    expect(h.onSwitch).toHaveBeenCalledWith("s1");

    open();
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(h.onOpenSettings).toHaveBeenCalledWith(burger());
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("closes a session through the tab close request without closing the menu", () => {
    setNarrow();
    const h = setup();
    open();
    fireEvent.click(screen.getByRole("button", { name: /Close tab "prod-db"/ }));
    expect(h.onRequestClose).toHaveBeenCalledWith("s1");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("closes on Escape and returns focus to the burger", () => {
    setNarrow();
    setup();
    open();
    expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(burger()).toHaveFocus();
  });

  it("closes on a tap outside the panel", () => {
    setNarrow();
    setup();
    open();
    fireEvent.click(screen.getByTestId("narrow-menu-backdrop"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(burger()).toHaveFocus();
  });

  it("closes when the window becomes wide", () => {
    setNarrow();
    setup();
    open();
    act(() => setWide());
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByText("wide header")).toBeInTheDocument();
  });
});
