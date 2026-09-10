// Spec 0053, Teil 1 ("Testbarkeit"): "Spaltenbreite ziehen -> Breite ändert
// sich, Mindestbreite wird nicht unterschritten, Wert überlebt Neustart."
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

// jsdom implementiert Pointer-Capture nicht (s. https://github.com/jsdom/jsdom/issues/2527) —
// `useDragResize` (Spec 0053) ruft `setPointerCapture`/`releasePointerCapture`
// auf jedem Drag-Handle auf; ohne diesen Polyfill würde jeder Klick auf ein
// Handle mit einer echten `TypeError` abbrechen, unabhängig vom eigentlich
// zu testenden Verhalten.
if (!Element.prototype.setPointerCapture) {
  Element.prototype.setPointerCapture = () => {};
  Element.prototype.releasePointerCapture = () => {};
}
import { sftpList } from "../api";
import {
  loadFileManagerColumnWidths,
  saveFileManagerColumnWidths,
} from "../layoutSettings";
import { testI18n } from "../testI18n";
import type { RemoteEntryDto } from "../types";
import { FileBrowserPanel } from "./FileBrowserPanel";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  sftpList: vi.fn(),
  sftpDelete: vi.fn(),
  sftpDownload: vi.fn(),
  sftpMkdir: vi.fn(),
  sftpRename: vi.fn(),
  sftpUpload: vi.fn(),
}));

vi.mock("../events", () => ({
  onSftpTransferStarted: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferFinished: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn(() => Promise.resolve(() => {})),
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("../layoutSettings", () => ({
  loadFileManagerColumnWidths: vi.fn(() => Promise.resolve({})),
  saveFileManagerColumnWidths: vi.fn(() => Promise.resolve()),
}));

const entry: RemoteEntryDto = {
  name: "readme.md",
  path: "readme.md",
  isDir: false,
  size: 1024,
  permissions: "rw-r--r--",
  modified: null,
};

function renderPanel() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <FileBrowserPanel sessionId="session-1" isVisible={true} />
    </I18nextProvider>,
  );
}

/** jsdom liefert für jedes Element `clientWidth: 0` (kein echtes Layout) —
 * `clampColumnWidth` braucht eine realistische Container-Breite, um
 * Wachstum (nicht nur Schrumpfen auf die Mindestbreite) überhaupt zu
 * erlauben. */
function stubContainerWidth(container: HTMLElement, width: number) {
  const scrollContainer = container.querySelector("table")?.parentElement;
  if (!scrollContainer) throw new Error("Tabellen-Container nicht gefunden");
  Object.defineProperty(scrollContainer, "clientWidth", { value: width, configurable: true });
}

describe("FileBrowserPanel column resizing (Spec 0053, Teil 1)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads persisted column widths on mount", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({ size: 200 });

    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);

    await waitFor(() => {
      const sizeCol = container.querySelectorAll("colgroup col")[1] as HTMLElement;
      expect(sizeCol.style.width).toBe("200px");
    });
  });

  it("dragging a column handle grows the column and persists the width on drag end", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);
    stubContainerWidth(container, 1000);

    const handle = screen.getByTestId("column-resize-size");
    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerMove(handle, { clientX: 140, buttons: 1 });
    fireEvent.pointerUp(handle, { clientX: 140 });

    await waitFor(() => {
      const sizeCol = container.querySelectorAll("colgroup col")[1] as HTMLElement;
      // Default 90px + 40px Delta = 130px.
      expect(sizeCol.style.width).toBe("130px");
    });
    expect(saveFileManagerColumnWidths).toHaveBeenCalledWith(
      expect.objectContaining({ size: 130 }),
    );
  });

  it("does not persist while merely dragging (only once the gesture ends)", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);
    stubContainerWidth(container, 1000);

    const handle = screen.getByTestId("column-resize-size");
    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerMove(handle, { clientX: 140, buttons: 1 });

    expect(saveFileManagerColumnWidths).not.toHaveBeenCalled();
  });

  it("does not persist on a plain click without any movement", async () => {
    // Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): die
    // vorherige "does not persist while merely dragging"-Prüfung deckte
    // nur den Zwischenzustand ab, nicht den eigentlich riskanteren Pfad —
    // ein abgeschlossenes `pointerdown`+`pointerup` ganz ohne `pointermove`
    // dazwischen (`useDragResize`s `movedRef`-Bedingung).
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/readme\.md/);

    const handle = screen.getByTestId("column-resize-size");
    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerUp(handle, { clientX: 100 });

    expect(saveFileManagerColumnWidths).not.toHaveBeenCalled();
  });

  it("caps growth so the Name column never drops below its own minimum width", async () => {
    // Spec-Reviewer-Fund: der bisherige Test nutzte einen so breiten
    // Container (1000px), dass der `NAME_MIN_WIDTH`-Klemm-Zweig in
    // `clampColumnWidth` nie erreicht wurde.
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);
    // 600px Container; andere Spalten (Rechte 90 + Geändert 150 + Aktionen
    // 36) + Name-Minimum (120) = 396px stehen fest, für "Größe" bleiben
    // maximal 600 - 396 = 204px.
    stubContainerWidth(container, 600);

    const handle = screen.getByTestId("column-resize-size");
    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerMove(handle, { clientX: 600, buttons: 1 }); // weit über das Maximum hinaus
    fireEvent.pointerUp(handle, { clientX: 600 });

    await waitFor(() => {
      const sizeCol = container.querySelectorAll("colgroup col")[1] as HTMLElement;
      expect(sizeCol.style.width).toBe("204px");
    });
  });

  it("does not shrink a column below its minimum width", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);
    stubContainerWidth(container, 1000);

    const handle = screen.getByTestId("column-resize-size");
    fireEvent.pointerDown(handle, { clientX: 100 });
    // Weit über die Mindestbreite (56px) hinaus nach links ziehen.
    fireEvent.pointerMove(handle, { clientX: -500, buttons: 1 });
    fireEvent.pointerUp(handle, { clientX: -500 });

    await waitFor(() => {
      const sizeCol = container.querySelectorAll("colgroup col")[1] as HTMLElement;
      expect(sizeCol.style.width).toBe("56px");
    });
  });

  it("existing navigation still works (unrelated to column resizing)", async () => {
    const dirEntry: RemoteEntryDto = { ...entry, name: "logs", path: "logs", isDir: true };
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    const dirButton = await screen.findByText(/logs/);
    fireEvent.click(dirButton);

    await waitFor(() => expect(sftpList).toHaveBeenCalledWith("session-1", "logs"));
  });
});
