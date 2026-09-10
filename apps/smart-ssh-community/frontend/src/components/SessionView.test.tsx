// Spec 0053, Teil 2 ("Testbarkeit"): "Bereichs-Splitter ziehen -> Aufteilung
// ändert sich, Mindestgrößen greifen, Wert überlebt Neustart" und "Kleines
// Fenster -> Layout bleibt bedienbar (Mindestgrößen)."
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadAiSshSplitWidthPx, saveAiSshSplitWidthPx } from "../layoutSettings";
import { SessionView } from "./SessionView";

vi.mock("./ChatPanel", () => ({ ChatPanel: () => <div>chat-panel-stub</div> }));
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
  lastResizeCallback(
    [{ target: lastResizeTarget, contentRect: { width } } as ResizeObserverEntry],
    new ResizeObserverMock(() => {}),
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
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(null);

    renderSessionView();

    await waitFor(() => expect(getRightPanel().style.width).toBe("420px"));
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
    fireEvent.pointerMove(handle, { clientX: 450 }); // 50px nach links -> +50 rechte Breite
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
    fireEvent.pointerMove(handle, { clientX: 1200 }); // weit nach rechts
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

  it("existing panel switcher (Terminal/Dateien) still works", async () => {
    vi.mocked(loadAiSshSplitWidthPx).mockResolvedValue(null);
    renderSessionView();

    expect(screen.getByText("terminal-stub")).toBeVisible();
    fireEvent.click(screen.getByText("Dateien"));

    expect(await screen.findByText("file-browser-stub")).toBeVisible();
  });
});
