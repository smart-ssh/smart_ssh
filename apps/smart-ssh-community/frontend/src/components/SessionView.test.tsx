// Spec 0103 / Issue #50: Eine MCP-Sitzung zeigt nur die Aktionskarten des
// externen Clients samt Ergebnis — ohne Chat-Eingabe, Terminal und
// Dateibrowser. Die Kind-Komponenten sind hier Attrappen: geprüft wird nur,
// was `SessionView` für welche Sitzung überhaupt rendert.
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { SessionView } from "./SessionView";

vi.mock("../events", () => ({
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../layoutSettings", () => ({
  loadAiSshSplitWidthPx: vi.fn(() => Promise.resolve(null)),
  saveAiSshSplitWidthPx: vi.fn(() => Promise.resolve()),
}));

vi.mock("./ChatPanel", () => ({
  ChatPanel: ({ readOnlyHint }: { readOnlyHint?: string }) => (
    <div data-testid="chat-panel">{readOnlyHint ?? "chat-input"}</div>
  ),
}));

vi.mock("./TerminalView", () => ({
  TerminalView: () => <div data-testid="terminal-view" />,
}));

vi.mock("./FileBrowserPanel", () => ({
  FileBrowserPanel: () => <div data-testid="file-browser" />,
}));

// jsdom kennt keinen `ResizeObserver`.
class NoopResizeObserver {
  observe() {}
  disconnect() {}
}
globalThis.ResizeObserver = NoopResizeObserver as unknown as typeof ResizeObserver;

function renderView(mcp: { clientName: string | null } | null) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <SessionView
        sessionId="s1"
        serverName="web-01"
        serverId="server-1"
        onRequestClose={vi.fn()}
        onActionSettled={vi.fn()}
        isActiveTab={false}
        mcp={mcp}
      />
    </I18nextProvider>,
  );
}

describe("SessionView (Spec 0103)", () => {
  it("renders an MCP session read-only: action cards, no terminal, no file browser", () => {
    renderView({ clientName: "Claude Code" });

    expect(screen.getByText("Claude Code @ web-01")).toBeInTheDocument();
    expect(screen.getByTestId("chat-panel")).toHaveTextContent(
      testI18n.t("sessionTabs.mcp.readOnlyHint"),
    );
    expect(screen.queryByTestId("terminal-view")).toBeNull();
    expect(screen.queryByTestId("file-browser")).toBeNull();
  });

  it("keeps the full layout for a user session", () => {
    renderView(null);

    expect(screen.getByText("web-01")).toBeInTheDocument();
    expect(screen.getByTestId("chat-panel")).toHaveTextContent("chat-input");
    expect(screen.getByTestId("terminal-view")).toBeInTheDocument();
    expect(screen.getByTestId("file-browser")).toBeInTheDocument();
  });
});
