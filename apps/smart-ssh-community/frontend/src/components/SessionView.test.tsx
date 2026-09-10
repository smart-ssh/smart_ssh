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
