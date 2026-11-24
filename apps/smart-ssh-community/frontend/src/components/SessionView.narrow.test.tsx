import type { TranslationKey } from "../i18nTypes";
// Issue #580, Spec 0113 §4: below 640 px the session view shows one pane at a
// time behind an AI / Terminal / Files tab strip.
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { useMemo, useState, type ReactNode } from "react";
import { NarrowSlotProvider, useNarrowSlot } from "../lib/narrowSlot";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { setNarrow, setWide } from "../test/matchMedia";
import { clampRightPanelWidthForDisplay, SessionView } from "./SessionView";

const fileBrowserVisible = vi.fn();
vi.mock("./ChatPanel", () => ({ ChatPanel: () => <div>chat-stub</div> }));
vi.mock("./TerminalView", () => ({ TerminalView: () => <div>terminal-stub</div> }));
vi.mock("./FileBrowserPanel", () => ({
  FileBrowserPanel: ({ isVisible }: { isVisible: boolean }) => {
    fileBrowserVisible(isVisible);
    return <div>file-browser-stub</div>;
  },
}));
vi.mock("../api/events", () => ({
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onAutoReconnectChanged: vi.fn(() => Promise.resolve(() => {})),
  onElevationAutoDisabled: vi.fn(() => Promise.resolve(() => {})),
  onElevationRevokedByProfile: vi.fn(() => Promise.resolve(() => {})),
}));
const reconnectSession = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    getServer: () => Promise.reject(new Error("no server")) as never,
    listGroups: () => Promise.resolve([]) as never,
    reconnectSession: (...a: unknown[]) => reconnectSession(...(a as [])) as never,
  }),
);
vi.mock("../settings/layoutSettings", () => ({
  loadAiSshSplitWidthPx: vi.fn(() => Promise.resolve(null)),
  saveAiSshSplitWidthPx: vi.fn(() => Promise.resolve()),
}));
vi.stubGlobal(
  "ResizeObserver",
  class {
    observe() {}
    unobserve() {}
    disconnect() {}
  },
);

const t = (key: TranslationKey) => testI18n.t(key);

// Mirrors App.tsx: the title bar provides the slot element through context.
function SlotHost({ children, late = false }: { children: ReactNode; late?: boolean }) {
  const [slot, setSlot] = useState<HTMLElement | null>(null);
  const value = useMemo(() => ({ slot, setSlot }), [slot]);
  return (
    <NarrowSlotProvider value={value}>
      {/* `late`: the slot span mounts only after a button press, i.e. in a later commit than the session view. */}
      <SlotSpan late={late} />
      {children}
    </NarrowSlotProvider>
  );
}
function SlotSpan({ late }: { late: boolean }) {
  const { setSlot } = useNarrowSlot();
  const [shown, setShown] = useState(!late);
  return (
    <>
      {late && <button onClick={() => setShown(true)}>mount-slot</button>}
      {shown && <div ref={setSlot} data-testid="narrow-session-actions" />}
    </>
  );
}

function renderView(
  props: { mcp?: { clientName: string } | null; onRequestClose?: () => void; slot?: boolean; late?: boolean } = {},
) {
  const view = (
    <SessionView
      sessionId="s1"
      serverName="srv"
      serverId="id"
      onRequestClose={props.onRequestClose ?? vi.fn()}
      onActionSettled={vi.fn()}
      isActiveTab={true}
      mcp={props.mcp ?? null}
    />
  );
  return render(
    <I18nextProvider i18n={testI18n}>
      {props.slot === false ? view : <SlotHost late={props.late}>{view}</SlotHost>}
    </I18nextProvider>,
  );
}

describe("SessionView narrow layout (#580)", () => {
  beforeEach(() => {
    setNarrow();
    fileBrowserVisible.mockClear();
  });
  afterEach(() => {
    setWide();
  });

  it("shows the tab strip with AI active and no separator", () => {
    renderView();
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((x) => x.textContent)).toEqual([t("session.aiTab"), t("session.terminalTab"), t("session.filesTab")]);
    expect(screen.getByRole("tab", { name: t("session.aiTab") })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("separator")).toBeNull();
    expect(screen.getByText("chat-stub").closest("[role=tabpanel]")).not.toHaveClass("hidden");
    expect(screen.getByText("terminal-stub").closest("[role=tabpanel]")).toHaveClass("hidden");
  });

  it("switches panes, keeps all mounted and only marks files visible on the Files tab", () => {
    renderView();
    expect(fileBrowserVisible).toHaveBeenLastCalledWith(false);
    fireEvent.click(screen.getByRole("tab", { name: t("session.terminalTab") }));
    expect(screen.getByText("terminal-stub").closest("[role=tabpanel]")).not.toHaveClass("hidden");
    expect(screen.getByText("chat-stub").closest("[role=tabpanel]")).toHaveClass("hidden");
    expect(fileBrowserVisible).toHaveBeenLastCalledWith(false);
    fireEvent.click(screen.getByRole("tab", { name: t("session.filesTab") }));
    expect(fileBrowserVisible).toHaveBeenLastCalledWith(true);
    fireEvent.keyDown(screen.getByRole("tab", { name: t("session.filesTab") }), { key: "ArrowLeft" });
    expect(screen.getByRole("tab", { name: t("session.terminalTab") })).toHaveAttribute("aria-selected", "true");
    expect(fileBrowserVisible).toHaveBeenLastCalledWith(false);
  });

  it("keeps the last narrow tab across a wide round trip and the wide choice in step", () => {
    renderView();
    fireEvent.click(screen.getByRole("tab", { name: t("session.filesTab") }));
    act(() => setWide());
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
    expect(screen.getByRole("button", { name: t("session.filesTab") })).toHaveAttribute("aria-pressed", "true");
    act(() => setNarrow());
    expect(screen.getByRole("tab", { name: t("session.filesTab") })).toHaveAttribute("aria-selected", "true");
  });

  it("reaches Disconnect through the overflow button", () => {
    const close = vi.fn();
    renderView({ onRequestClose: close });
    expect(screen.queryByText(t("session.disconnect"))).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: t("session.moreActions") }));
    fireEvent.click(screen.getByText(t("session.disconnect")));
    expect(close).toHaveBeenCalled();
  });

  it("renders the ⋯ button in the title bar slot and no second header row (#1387)", () => {
    renderView();
    const slot = document.querySelector('[data-testid="narrow-session-actions"]') as HTMLElement;
    expect(slot).toContainElement(screen.getByRole("button", { name: t("session.moreActions") }));
    expect(document.querySelector("header")).toBeNull();
  });

  it("finds the ⋯ button in a slot that mounts in a later commit than the session (#1461)", () => {
    renderView({ late: true });
    expect(screen.queryByRole("button", { name: t("session.moreActions") })).toBeNull();
    fireEvent.click(screen.getByText("mount-slot"));
    const slot = document.querySelector('[data-testid="narrow-session-actions"]') as HTMLElement;
    expect(slot).toContainElement(screen.getByRole("button", { name: t("session.moreActions") }));
  });

  it("offers a Reconnect button in the status strip after a disconnect (#1387)", async () => {
    const events = await import("../api/events");
    renderView();
    const handler = vi.mocked(events.onConnectionStatusChanged).mock.calls.at(-1)![0];
    act(() => handler({ sessionId: "s1", status: "disconnected", reason: null } as never));
    const strip = await screen.findByTestId("session-status-strip");
    const button = within(strip).getByRole("button", { name: t("session.reconnect") });
    fireEvent.click(button);
    await waitFor(() => expect(reconnectSession).toHaveBeenCalledWith("s1"));
    await waitFor(() => expect(screen.queryByTestId("session-status-strip")).toBeNull());
  });

  it("shows no tab strip for an MCP session", () => {
    renderView({ mcp: { clientName: "c" } });
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
    expect(screen.getByText("chat-stub")).toBeInTheDocument();
  });

  it("never lets the right panel exceed the window in the wide fallback", () => {
    for (const w of [100, 500, 665, 700, 1000]) {
      expect(clampRightPanelWidthForDisplay(420, w)).toBeLessThanOrEqual(w);
    }
  });
});
