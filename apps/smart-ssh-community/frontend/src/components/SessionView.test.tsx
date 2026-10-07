// Spec 0053, Teil 2 ("Testbarkeit"): "Bereichs-Splitter ziehen -> Aufteilung
// ändert sich, Mindestgrößen greifen, Wert überlebt Neustart" und "Kleines
// Fenster -> Layout bleibt bedienbar (Mindestgrößen)."
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadAiSshSplitWidthPx, saveAiSshSplitWidthPx } from "../layoutSettings";
import { testI18n } from "../testI18n";
import { SessionView } from "./SessionView";

vi.mock("./ChatPanel", () => ({
  ChatPanel: ({ readOnlyHint }: { readOnlyHint?: string }) => (
    <div data-testid="chat-panel-stub">{readOnlyHint ?? "chat-panel-stub"}</div>
  ),
}));
vi.mock("./TerminalView", () => ({ TerminalView: () => <div>terminal-stub</div> }));
vi.mock("./FileBrowserPanel", () => ({ FileBrowserPanel: () => <div>file-browser-stub</div> }));

vi.mock("../events", () => ({
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../layoutSettings", () => ({
  loadAiSshSplitWidthPx: vi.fn(() => Promise.resolve(null)),
  saveAiSshSplitWidthPx: vi.fn(() => Promise.resolve()),
}));

// jsdom implementiert Pointer-Capture nicht — s. identischer Kommentar in
// FileBrowserPanel.test.tsx.
if (!Element.prototype.setPointerCapture) {
  Element.prototype.setPointerCapture = () => {};
  Element.prototype.releasePointerCapture = () => {};
}

// jsdom implementiert `ResizeObserver` nicht. Dieser Ersatz ruft `callback`
// nie von sich aus auf (das würde beim `observe()`-Aufruf ohnehin nur das
// standardmäßige `clientWidth: 0` von jsdom liefern) — Tests, die die
// Container-Breite kontrollieren wollen, tun das explizit über
// `triggerResize`.
let lastResizeCallback: ResizeObserverCallback | null = null;
let lastResizeTarget: Element | null = null;
class ResizeObserverMock implements ResizeObserver {
  constructor(callback: ResizeObserverCallback) {
    lastResizeCallback = callback;
  }
  observe(target: Element) {
    lastResizeTarget = target;
  }
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal("ResizeObserver", ResizeObserverMock);

function triggerResize(width: number) {
  if (!lastResizeCallback || !lastResizeTarget) {
    throw new Error("kein ResizeObserver registriert");
  }
  // Kein `new ResizeObserverMock(...)` als zweites Argument: dessen
  // Konstruktor überschreibt `lastResizeCallback` als Seiteneffekt (s.
  // oben) — ein zweiter `triggerResize`-Aufruf in demselben Test hätte
  // damit den echten, registrierten Callback stillschweigend durch einen
  // No-op ersetzt und wäre wirkungslos verpufft. Ein reiner Objekt-Cast
  // reicht als zweites Argument, es wird von der Komponente ohnehin nicht
  // ausgewertet.
  lastResizeCallback(
    [{ target: lastResizeTarget, contentRect: { width } } as ResizeObserverEntry],
    {} as ResizeObserver,
  );
}

function renderSessionView() {
  return render(
    <SessionView
      sessionId="session-1"
      serverName="srv1"
      serverId="server-1"
      onRequestClose={vi.fn()}
      onActionSettled={vi.fn()}
      isActiveTab={true}
    />,
  );
}

/** Findet den rechten (SSH-/SFTP-)Bereich über seinen einzigen "Terminal"-
 * Umschalter-Text, dann dessen Container mit der `style`-gesetzten Breite. */
function getRightPanel(): HTMLElement {
  const terminalToggle = screen.getByText("Terminal");
  // header (h-8 Umschalter-Leiste) -> rechter Bereich-Container
  return terminalToggle.closest("div")!.parentElement as HTMLElement;
}

describe("SessionView AI/SSH split (Spec 0053, Teil 2)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("uses the default width when nothing is persisted", async () => {
    // Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): der
    // Ausgangs-State ist bereits 420px, ohne die `toHaveBeenCalled`-
    // Prüfung würde dieser Test auch dann noch grün sein, wenn
    // `loadAiSshSplitWidthPx` nie aufgerufen würde.
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(null);

    renderSessionView();

    await waitFor(() => expect(loadAiSshSplitWidthPx).toHaveBeenCalled());
    expect(getRightPanel().style.width).toBe("420px");
  });

  it("loads a persisted split width on mount", async () => {
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(500);

    renderSessionView();

    await waitFor(() => expect(getRightPanel().style.width).toBe("500px"));
  });

  it("dragging the divider left grows the right panel and persists only on drag end", async () => {
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(null);

    renderSessionView();
    await waitFor(() => expect(getRightPanel().style.width).toBe("420px"));

    const handle = screen.getByLabelText("Bereichsaufteilung");
    Object.defineProperty(handle.parentElement, "clientWidth", {
      value: 1400,
      configurable: true,
    });

    fireEvent.pointerDown(handle, { clientX: 500 });
    fireEvent.pointerMove(handle, { clientX: 450, buttons: 1 }); // 50px nach links -> +50 rechte Breite
    expect(saveAiSshSplitWidthPx).not.toHaveBeenCalled();
    fireEvent.pointerUp(handle, { clientX: 450 });

    await waitFor(() => expect(getRightPanel().style.width).toBe("470px"));
    expect(saveAiSshSplitWidthPx).toHaveBeenCalledWith(470);
  });

  it("does not let the right panel shrink below its minimum width", async () => {
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(null);

    renderSessionView();
    await waitFor(() => expect(getRightPanel().style.width).toBe("420px"));

    const handle = screen.getByLabelText("Bereichsaufteilung");
    Object.defineProperty(handle.parentElement, "clientWidth", {
      value: 1400,
      configurable: true,
    });

    fireEvent.pointerDown(handle, { clientX: 500 });
    fireEvent.pointerMove(handle, { clientX: 1200, buttons: 1 }); // weit nach rechts
    fireEvent.pointerUp(handle, { clientX: 1200 });

    await waitFor(() => expect(getRightPanel().style.width).toBe("300px"));
  });

  it("falls back to the default split on a window too small for both minimums", async () => {
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(900);

    renderSessionView();
    await waitFor(() => expect(getRightPanel().style.width).toBe("900px"));

    triggerResize(500); // < LEFT_MIN (360) + RIGHT_MIN (300) + HANDLE (6)

    await waitFor(() => expect(getRightPanel().style.width).toBe("420px"));
  });

  it("does not lose the stored preference when dragging in a too-small window", async () => {
    // Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): vorher wurde
    // im "zu klein"-Regime `preferredRightWidth` selbst (nicht nur die
    // Anzeige) auf den Default gesetzt UND persistiert — ein einziges
    // Ruckeln am Divider in einem schmalen Fenster löschte damit eine
    // gespeicherte Präferenz dauerhaft.
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(900);

    renderSessionView();
    await waitFor(() => expect(getRightPanel().style.width).toBe("900px"));
    triggerResize(500); // < LEFT_MIN (360) + RIGHT_MIN (300) + HANDLE (6)
    await waitFor(() => expect(getRightPanel().style.width).toBe("420px"));

    const handle = screen.getByLabelText("Bereichsaufteilung");
    Object.defineProperty(handle.parentElement, "clientWidth", {
      value: 500,
      configurable: true,
    });
    fireEvent.pointerDown(handle, { clientX: 500 });
    fireEvent.pointerMove(handle, { clientX: 480, buttons: 1 });
    fireEvent.pointerUp(handle, { clientX: 480 });

    // Die Geste selbst bewegte den Zeiger, `onDragEnd` feuert also (s.
    // `useDragResize`s `movedRef`) — entscheidend ist, dass ein dabei
    // ausgelöster Speichervorgang trotzdem die unveränderte 900px-
    // Präferenz schreibt, nie einen im zu-kleinen-Regime verfälschten Wert.
    if (vi.mocked(saveAiSshSplitWidthPx).mock.calls.length > 0) {
      expect(saveAiSshSplitWidthPx).toHaveBeenCalledWith(900);
    }

    // Fenster wieder vergrößern -> die ursprüngliche 900px-Präferenz muss
    // zurückkommen, nicht ein durch das Ruckeln veränderter Wert.
    triggerResize(1400);
    await waitFor(() => expect(getRightPanel().style.width).toBe("900px"));
  });

  it("existing panel switcher (Terminal/Dateien) still works", async () => {
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(null);
    renderSessionView();

    expect(screen.getByText("terminal-stub")).toBeVisible();
    fireEvent.click(screen.getByText("Dateien"));

    expect(await screen.findByText("file-browser-stub")).toBeVisible();
  });
});

// Spec 0104 / Issue #50: Eine MCP-Sitzung zeigt nur die Aktionskarten des
// externen Clients samt Ergebnis — ohne Chat-Eingabe, Terminal und
// Dateibrowser.
describe("SessionView MCP session (Spec 0104)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  function renderMcpView(clientName: string | null) {
    return render(
      <I18nextProvider i18n={testI18n}>
        <SessionView
          sessionId="session-1"
          serverName="web-01"
          serverId="server-1"
          onRequestClose={vi.fn()}
          onActionSettled={vi.fn()}
          isActiveTab={false}
          mcp={{ clientName }}
        />
      </I18nextProvider>,
    );
  }

  it("renders an MCP session read-only: action cards, no terminal, no file browser", () => {
    renderMcpView("Claude Code");

    expect(screen.getByText("Claude Code @ web-01")).toBeInTheDocument();
    expect(screen.getByTestId("chat-panel-stub")).toHaveTextContent(
      testI18n.t("sessionTabs.mcp.readOnlyHint"),
    );
    expect(screen.queryByText("terminal-stub")).toBeNull();
    expect(screen.queryByText("file-browser-stub")).toBeNull();
  });

  it("keeps the full layout with chat input for a user session", () => {
    renderSessionView();

    expect(screen.getByTestId("chat-panel-stub")).toHaveTextContent("chat-panel-stub");
    expect(screen.getByText("terminal-stub")).toBeInTheDocument();
  });
});
