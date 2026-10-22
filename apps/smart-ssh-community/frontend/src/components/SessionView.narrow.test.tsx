// Issue #580, Spec 0113 §4: below 640 px the session view shows one pane at a
// time behind an AI / Terminal / Files tab strip.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
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
vi.mock("../events", () => ({
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onAutoReconnectChanged: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("../layoutSettings", () => ({
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

const t = (key: string) => testI18n.t(key);

function renderView(props: { mcp?: { clientName: string } | null; onRequestClose?: () => void } = {}) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <SessionView
        sessionId="s1"
        serverName="srv"
        serverId="id"
        onRequestClose={props.onRequestClose ?? vi.fn()}
        onActionSettled={vi.fn()}
        isActiveTab={true}
        mcp={props.mcp ?? null}
      />
    </I18nextProvider>,
  );
}

describe("SessionView narrow layout (#580)", () => {
  beforeEach(() => {
    setNarrow();
    fileBrowserVisible.mockClear();
  });
  afterEach(() => setWide());

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
