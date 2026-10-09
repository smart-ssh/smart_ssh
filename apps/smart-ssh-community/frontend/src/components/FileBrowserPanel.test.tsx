// Spec 0053, Teil 1 ("Testbarkeit"): "Spaltenbreite ziehen -> Breite ändert
// sich, Mindestbreite wird nicht unterschritten, Wert überlebt Neustart."
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  claimDroppedPaths,
  closeEditSession,
  localFileMtime,
  pickUploadFiles,
  readLocalTextPreview,
  sftpChmod,
  sftpDelete,
  sftpDeletePreview,
  sftpDownload,
  sftpDownloadDefault,
  sftpDownloadDir,
  sftpElevationDisable,
  sftpElevationEnable,
  sftpExists,
  sftpList,
  sftpOpenForEditing,
  sftpReadText,
  sftpRename,
  sftpStat,
  sftpUpload,
} from "../api";
import { onConnectionStatusChanged, onSftpTransferStarted } from "../events";
import {
  loadFileManagerColumnWidths,
  saveFileManagerColumnWidths,
} from "../layoutSettings";
import { testI18n } from "../testI18n";
import { showToast } from "../toastBus";
import type { RemoteEntryDto, SftpTransferStartedEvent } from "../types";
import { FileBrowserPanel } from "./FileBrowserPanel";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  sftpList: vi.fn(),
  sftpDelete: vi.fn(),
  sftpDeletePreview: vi.fn(),
  sftpDownload: vi.fn(),
  sftpDownloadDefault: vi.fn(),
  sftpDownloadDir: vi.fn(),
  sftpElevationDisable: vi.fn(() => Promise.resolve()),
  sftpElevationEnable: vi.fn(),
  sftpElevationStatus: vi.fn(() => Promise.resolve(null)),
  sftpMkdir: vi.fn(),
  sftpReadText: vi.fn(),
  sftpRename: vi.fn(),
  sftpUpload: vi.fn(),
  sftpExists: vi.fn(),
  sftpChmod: vi.fn(),
  readLocalTextPreview: vi.fn(),
  // Issue #89: the backend hands out the paths of the native drop it
  // captured — here the paths of the last simulated drop event.
  claimDroppedPaths: vi.fn(() => Promise.resolve(nativeDrop.paths)),
  pickUploadFiles: vi.fn(),
  sftpStat: vi.fn(),
  sftpOpenForEditing: vi.fn(),
  localFileMtime: vi.fn(),
  closeEditSession: vi.fn(),
}));

vi.mock("../events", () => ({
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferStarted: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferFinished: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn(),
  revealItemInDir: vi.fn(() => Promise.resolve()),
}));

vi.mock("../toastBus", () => ({
  showToast: vi.fn(),
}));

vi.mock("../fileTypeSettings", () => ({
  loadFileTypeApps: vi.fn(() => Promise.resolve({})),
  appForFileName: () => null,
}));

// Issue #89: what the backend saw in the native drop event. The webview's
// event payload is not trusted; the panel claims these paths instead.
const nativeDrop = vi.hoisted(() => ({ paths: [] as string[] }));

const dragDrop = vi.hoisted(() => ({
  handler: null as null | ((event: { payload: { type: string; paths?: string[] } }) => void),
  // Issue #29: how often a listener was registered / removed, so tests can
  // assert that a channel switch or a navigation does not re-register it.
  registrations: 0,
  unlistens: 0,
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((h) => {
      dragDrop.handler = (event: { payload: { type: string; paths?: string[] } }) => {
        if (event.payload.type === "drop") nativeDrop.paths = event.payload.paths ?? [];
        h(event);
      };
      dragDrop.registrations += 1;
      return Promise.resolve(() => {
        dragDrop.unlistens += 1;
      });
    }),
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
  permissionsOctal: 0o644,
  uid: 1000,
  gid: 1000,
  owner: null,
  group: null,
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

    await waitFor(() => expect(sftpList).toHaveBeenCalledWith("session-1", "logs", null));
  });
});

describe("FileBrowserPanel row action menu (Spec 0054, Teil 0 — Bug-Fix)", () => {
  // Regressionstest für den Drei-Punkte-Menü-Bug: der dokumentweite
  // "Klick-außerhalb-schließt-das-Menü"-Effekt hängte einen rohen
  // `document`-Click-Listener ein, der bei jedem weiteren Klick den
  // aktuellen `menuFor`-State auf `null` überschrieb — auch wenn dieser
  // Klick eigentlich das Menü eines ANDEREN Eintrags öffnen sollte. Vor dem
  // Fix (`stopPropagation` im Trigger-Button) blieb das zweite Menü also
  // geschlossen statt sich zu öffnen; das wurde manuell gegen den
  // unfixierten Stand verifiziert (Test schlägt ohne `stopPropagation` fehl).
  const entryA: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };
  const entryB: RemoteEntryDto = { ...entry, name: "b.txt", path: "b.txt" };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("switches the open menu to a different row instead of closing both", async () => {
    vi.mocked(sftpList).mockResolvedValue([entryA, entryB]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);

    const triggers = screen.getAllByRole("button", { name: "⋮" });
    expect(triggers).toHaveLength(2);

    fireEvent.click(triggers[0]);
    expect(screen.getAllByText("Löschen")).toHaveLength(1);

    fireEvent.click(triggers[1]);
    // Vor dem Fix: beide setMenuFor-Aufrufe (onClick von B, dann der
    // dokumentweite Listener aus dem Effekt für A) liefen im selben
    // Klick-Batch, der zweite (unbedingt `null`) gewann — kein Menü blieb
    // offen. Nach dem Fix bleibt genau ein Menü offen: das von B.
    expect(screen.getAllByText("Löschen")).toHaveLength(1);
  });

  it("still closes the menu on a genuine click outside", async () => {
    vi.mocked(sftpList).mockResolvedValue([entryA, entryB]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);

    const triggers = screen.getAllByRole("button", { name: "⋮" });
    fireEvent.click(triggers[0]);
    expect(screen.getAllByText("Löschen")).toHaveLength(1);

    fireEvent.click(document.body);
    expect(screen.queryAllByText("Löschen")).toHaveLength(0);
  });
});

describe("FileBrowserPanel context menu + read-only actions (Spec 0054, Teil 1+2)", () => {
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };
  const dirEntry: RemoteEntryDto = {
    ...entry,
    name: "logs",
    path: "logs",
    isDir: true,
  };

  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      configurable: true,
    });
  });

  it("right-click opens the same actions as the three-dots menu", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    const row = (await screen.findByText(/a\.txt/)).closest("tr")!;
    fireEvent.contextMenu(row);

    expect(screen.getByText("Herunterladen")).toBeVisible();
    expect(screen.getByText("Herunterladen nach…")).toBeVisible();
    expect(screen.getByText("Dateiinhalt kopieren")).toBeVisible();
    expect(screen.getByText("Pfad kopieren")).toBeVisible();
    expect(screen.getByText("Eigenschaften")).toBeVisible();
    expect(screen.getByText("Aktualisieren")).toBeVisible();
    expect(screen.getByText("Umbenennen")).toBeVisible();
    expect(screen.getByText("Löschen")).toBeVisible();
  });

  it("hides content-copy for directories (Löschen/Herunterladen work for both, Spec 0054 Teil 3)", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    const row = (await screen.findByText(/logs/)).closest("tr")!;
    fireEvent.contextMenu(row);

    expect(screen.queryByText("Dateiinhalt kopieren")).not.toBeInTheDocument();
    // Löschen/Herunterladen sind seit Spec 0054, Teil 3 auch für Ordner
    // verfügbar (rekursiv, s. Backend) — nur "Dateiinhalt kopieren" bleibt
    // dateispezifisch.
    expect(screen.getByText("Löschen")).toBeVisible();
    expect(screen.getByText("Herunterladen")).toBeVisible();
  });

  it("'Herunterladen' calls the no-dialog default-directory download", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpDownloadDefault).mockResolvedValue({
      localPath: "/Users/test/Downloads/a.txt",
      isDir: false,
      fileCount: 1,
    });

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Herunterladen"));

    expect(sftpDownloadDefault).toHaveBeenCalledWith("session-1", "a.txt", null);
  });

  it("'Herunterladen nach…' uses the folder dialog for directories, file dialog for files", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpDownloadDir).mockResolvedValue(null);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Herunterladen nach…"));

    expect(sftpDownloadDir).toHaveBeenCalledWith("session-1", "logs", "Zielordner wählen", null);
    expect(sftpDownload).not.toHaveBeenCalled();
  });

  it("copies the path to the clipboard", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Pfad kopieren"));

    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledWith("a.txt"));
    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ kind: "success", message: "Pfad in die Zwischenablage kopiert" }),
      ),
    );
  });

  it("copies the file content to the clipboard via sftp_read_text", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpReadText).mockResolvedValue("hallo welt");

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));

    await waitFor(() =>
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith("hallo welt"),
    );
    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "success",
          message: "Inhalt von „a.txt“ in die Zwischenablage kopiert",
        }),
      ),
    );
  });

  it("shows a toast with the backend error when the file is not text", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpReadText).mockRejectedValue("Datei ist keine Textdatei (kein gültiges UTF-8)");

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "error",
          message: expect.stringContaining("Datei ist keine Textdatei (kein gültiges UTF-8)"),
        }),
      ),
    );
    expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  });

  it("shows the properties dialog with numeric and symbolic permissions", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Eigenschaften"));

    expect(await screen.findByText("Eigenschaften")).toBeVisible();
    // "rw-r--r--" steht sowohl in der Tabellenspalte als auch im Dialog —
    // die numerische Darstellung ("644") ist eindeutig nur im Dialog.
    expect(screen.getAllByText("rw-r--r--").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("644")).toBeVisible();
  });

  it("'Aktualisieren' reloads the current directory", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Aktualisieren"));

    await waitFor(() => expect(sftpList).toHaveBeenCalledTimes(2));
    expect(sftpList).toHaveBeenLastCalledWith("session-1", ".", null);
  });
});

describe("FileBrowserPanel server-modifying actions (Spec 0054, Teil 3)", () => {
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };
  const dirEntry: RemoteEntryDto = {
    ...entry,
    name: "logs",
    path: "logs",
    isDir: true,
    permissionsOctal: 0o755,
    permissions: "rwxr-xr-x",
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("chmod dialog toggles a bit and applies the resulting octal mode", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(1);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    expect(await screen.findByText("Rechte bearbeiten")).toBeVisible();
    // fileEntry ist 644 (rw-r--r--) — "Owner Schreiben" (0o200) abwählen.
    fireEvent.click(screen.getByLabelText("Owner 128"));
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "a.txt", 0o444, false, null),
    );
  });

  it("names the otherwise empty header cells of the file table and chmod matrix (issue #112)", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);
    expect(screen.getByRole("columnheader", { name: "Aktionen" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));
    expect(await screen.findByRole("columnheader", { name: "Gilt für" })).toBeInTheDocument();
  });

  it("chmod dialog's numeric input overrides the checkbox matrix", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(1);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    const numeric = await screen.findByDisplayValue("0644");
    fireEvent.change(numeric, { target: { value: "0700" } });
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "a.txt", 0o700, false, null),
    );
  });

  it("chmod dialog's numeric field shows and preserves the setuid/setgid/sticky bit", async () => {
    // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): mit einem
    // auf 3 Ziffern begrenzten Eingabefeld war die "vierte Ziffer"
    // (setuid/setgid/sticky) unsichtbar — jede Bearbeitung der Zahl löschte
    // sie still. Mit setuid (0o4755) muss das Feld "4755" zeigen, und ein
    // Übernehmen mit unverändertem Text darf das Bit nicht verlieren.
    const setuidEntry: RemoteEntryDto = {
      ...entry,
      name: "suid-bin",
      path: "suid-bin",
      permissionsOctal: 0o4755,
    };
    vi.mocked(sftpList).mockResolvedValue([setuidEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(1);

    renderPanel();
    await screen.findByText(/suid-bin/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    expect(await screen.findByDisplayValue("4755")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "suid-bin", 0o4755, false, null),
    );
  });

  it("chmod dialog shows a recursive checkbox only for directories", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(1);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    const recursiveLabel = await screen.findByText(/Rekursiv/);
    fireEvent.click(recursiveLabel.closest("label")!.querySelector("input")!);
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "logs", 0o755, true, null),
    );
  });

  it("delete confirmation shows a file/dir count preview for folders", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpDeletePreview).mockResolvedValue({ fileCount: 3, dirCount: 2 });

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Löschen"));

    expect(await screen.findByText(/Ordner löschen/)).toBeVisible();
    await waitFor(() => expect(sftpDeletePreview).toHaveBeenCalledWith("session-1", "logs", null));
    expect(await screen.findByText("3")).toBeVisible();
    expect(screen.getByText("2")).toBeVisible();
    // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): die
    // härteste Spec-Invariante hier ist "Bestätigung Pflicht" — ohne diese
    // Prüfung würde ein Test, der `sftpDelete` versehentlich schon beim
    // Öffnen des Dialogs auslöst, nicht auffallen.
    expect(sftpDelete).not.toHaveBeenCalled();
  });

  it("rename checks for a collision and asks before overwriting", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(true);
    vi.mocked(sftpRename).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Umbenennen"));

    const input = screen.getByDisplayValue("a.txt");
    fireEvent.change(input, { target: { value: "b.txt" } });
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() => expect(sftpExists).toHaveBeenCalledWith("session-1", "b.txt", null));
    expect(await screen.findByText("Ziel existiert bereits")).toBeVisible();
    expect(sftpRename).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Überschreiben"));
    await waitFor(() => expect(sftpRename).toHaveBeenCalledWith("session-1", "a.txt", "b.txt", null));
  });

  it("rename proceeds directly when there is no collision", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpRename).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Umbenennen"));

    const input = screen.getByDisplayValue("a.txt");
    fireEvent.change(input, { target: { value: "b.txt" } });
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() => expect(sftpRename).toHaveBeenCalledWith("session-1", "a.txt", "b.txt", null));
    expect(screen.queryByText("Ziel existiert bereits")).not.toBeInTheDocument();
  });

  it("cut + paste moves the entry into a different directory via sftp_rename", async () => {
    // Ausschneiden in "." (enthält a.txt + logs/), dann in "logs" navigieren
    // und dort einfügen — Einfügen in dasselbe Verzeichnis, in dem die
    // Datei bereits liegt, ist bewusst ein No-op (s. `handlePaste`s
    // "bereits hier"-Kommentar), dieser Test prüft den eigentlichen
    // Verschiebe-Fall in ein ANDERES Verzeichnis.
    vi.mocked(sftpList).mockImplementation((_session, requestedPath) =>
      Promise.resolve(requestedPath === "logs" ? [] : [fileEntry, dirEntry]),
    );
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpRename).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    // Mock liefert `[fileEntry, dirEntry]` in dieser Reihenfolge (echtes
    // Sortieren übernimmt das Backend, nicht diese Komponente) — Zeile 0
    // ist also a.txt.
    const rows = screen.getAllByRole("button", { name: "⋮" });
    fireEvent.click(rows[0]);
    fireEvent.click(screen.getByText("Ausschneiden"));

    fireEvent.click(await screen.findByText(/logs/));
    await screen.findByText("(leeres Verzeichnis)");

    fireEvent.click(screen.getByText("Einfügen"));

    await waitFor(() => expect(sftpExists).toHaveBeenCalledWith("session-1", "logs/a.txt", null));
    expect(sftpRename).toHaveBeenCalledWith("session-1", "a.txt", "logs/a.txt", null);
  });

  it("upload onto an existing text file shows a diff, confirming uploads it", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(true);
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpReadText).mockResolvedValue("alt");
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    vi.mocked(pickUploadFiles).mockResolvedValue(["/local/a.txt"]);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Hochladen"));

    expect(await screen.findByText("Datei überschreiben?")).toBeVisible();
    expect(sftpUpload).not.toHaveBeenCalled();
    // Diff-Zeilen aus `NoteDiffPreview` (entfernt "alt", hinzugefügt "neu").
    expect(screen.getByText("alt")).toBeVisible();
    expect(screen.getByText("neu")).toBeVisible();

    fireEvent.click(screen.getByText("Überschreiben"));
    await waitFor(() =>
      expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/a.txt", "a.txt", null),
    );
    // Issue #89: the preview names its session, so the backend can check the
    // path against that session's grants.
    expect(readLocalTextPreview).toHaveBeenCalledWith("session-1", "/local/a.txt");
  });

  // Issue #89: the upload button asks the backend to show the dialog; the
  // webview never opens a file dialog itself.
  it("the upload button uses the backend dialog and uploads what it returns", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    vi.mocked(pickUploadFiles).mockResolvedValue(["/local/picked.txt"]);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Hochladen"));

    await waitFor(() =>
      expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/picked.txt", "picked.txt", null),
    );
    expect(pickUploadFiles).toHaveBeenCalledWith("session-1", "Datei(en) hochladen");
    expect(open).not.toHaveBeenCalled();
  });

  it("cancelling the backend dialog uploads nothing", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(pickUploadFiles).mockResolvedValue(null);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Hochladen"));

    await waitFor(() => expect(pickUploadFiles).toHaveBeenCalledTimes(1));
    expect(sftpExists).not.toHaveBeenCalled();
    expect(sftpUpload).not.toHaveBeenCalled();
  });

  it("upload to a new path skips the conflict dialog entirely", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    vi.mocked(pickUploadFiles).mockResolvedValue(["/local/new.txt"]);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Hochladen"));

    await waitFor(() =>
      expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/new.txt", "new.txt", null),
    );
    expect(screen.queryByText("Datei überschreiben?")).not.toBeInTheDocument();
    expect(readLocalTextPreview).not.toHaveBeenCalled();
  });
});

describe("FileBrowserPanel 'Lokal öffnen' flow (Spec 0054, Teil 4)", () => {
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("'Lokal öffnen…' downloads the file and shows an editing banner", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/a.txt",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Lokal öffnen…"));

    await waitFor(() =>
      expect(sftpOpenForEditing).toHaveBeenCalledWith("session-1", "a.txt", null),
    );
    expect(await screen.findByText(/wird lokal bearbeitet/)).toBeVisible();
  });

  it("'Bearbeitung beenden' ends the session and removes the banner", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/a.txt",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");
    vi.mocked(closeEditSession).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Lokal öffnen…"));
    await screen.findByText(/wird lokal bearbeitet/);

    fireEvent.click(screen.getByText("Bearbeitung beenden"));

    expect(closeEditSession).toHaveBeenCalledWith("session-1", "/tmp/edit/a.txt");
    expect(screen.queryByText(/wird lokal bearbeitet/)).not.toBeInTheDocument();
  });
});

describe("FileBrowserPanel result toasts (Spec 0067, Teil B)", () => {
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };
  const dirEntry: RemoteEntryDto = { ...entry, name: "logs", path: "logs", isDir: true };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
  });

  const lastToast = () => vi.mocked(showToast).mock.calls.at(-1)?.[0];

  it("download: success toast names file and target dir and offers to reveal it", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(sftpDownloadDefault).mockResolvedValue({
      localPath: "/Users/test/Downloads/a.txt",
      isDir: false,
      fileCount: 1,
    });

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Herunterladen"));

    await waitFor(() =>
      expect(lastToast()).toMatchObject({
        kind: "success",
        message: "„a.txt“ heruntergeladen nach /Users/test/Downloads",
      }),
    );
    lastToast()!.action!.onClick();
    expect(revealItemInDir).toHaveBeenCalledWith("/Users/test/Downloads/a.txt");
  });

  it("folder download: one summary toast with the file count", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(sftpDownloadDefault).mockResolvedValue({
      localPath: "/Users/test/Downloads/logs",
      isDir: true,
      fileCount: 42,
    });

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Herunterladen"));

    await waitFor(() =>
      expect(lastToast()).toMatchObject({
        kind: "success",
        message: "Ordner „logs“ heruntergeladen — 42 Dateien",
      }),
    );
    expect(showToast).toHaveBeenCalledTimes(1);
  });

  it("download failure: error toast with the reason", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(sftpDownloadDefault).mockRejectedValue({
      message: "Keine Berechtigung",
      code: null,
    });

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Herunterladen"));

    await waitFor(() =>
      expect(lastToast()).toMatchObject({
        kind: "error",
        message: expect.stringContaining("Herunterladen von „a.txt“ fehlgeschlagen"),
      }),
    );
  });

  it("uploading several files shows one summary toast, not one per file", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    vi.mocked(pickUploadFiles).mockResolvedValue(["/local/x.txt", "/local/y.txt", "/local/z.txt"]);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Hochladen"));

    await waitFor(() =>
      expect(lastToast()).toMatchObject({ kind: "success", message: "3 Dateien hochgeladen" }),
    );
    expect(showToast).toHaveBeenCalledTimes(1);
  });

  it("recursive chmod reports how many entries were changed", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(sftpChmod).mockResolvedValue(17);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));
    const recursiveLabel = await screen.findByText(/Rekursiv/);
    fireEvent.click(recursiveLabel.closest("label")!.querySelector("input")!);
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(lastToast()).toMatchObject({
        kind: "success",
        message: "Rechte auf 644 gesetzt — 17 Einträge in „logs“",
      }),
    );
  });

  it("deleting a folder reports the counts from the preview", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(sftpDeletePreview).mockResolvedValue({ fileCount: 3, dirCount: 2 });
    vi.mocked(sftpDelete).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Löschen"));
    await screen.findByText("3");
    const confirm = screen.getAllByText("Löschen").at(-1)!;
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(lastToast()).toMatchObject({
        kind: "success",
        message: "Ordner „logs“ gelöscht — 3 Dateien, 1 Unterordner",
      }),
    );
  });
});

describe("FileBrowserPanel elevated mode (Spec 0067, A5)", () => {
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
  });

  const enableElevation = async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: null,
    });
    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));
    await screen.findByRole("alert");
  };

  it("always starts in normal mode and closes a leftover elevated channel on open", async () => {
    renderPanel();
    await screen.findByText(/a\.txt/);

    expect(sftpElevationDisable).toHaveBeenCalledWith("session-1");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(sftpList).toHaveBeenCalledWith("session-1", ".", null);
  });

  it("marks the browser unmistakably and routes actions through the elevated channel", async () => {
    await enableElevation();

    expect(screen.getByRole("alert")).toHaveTextContent("root");
    expect(document.querySelector('[data-elevated="true"]')).not.toBeNull();
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", ".", "root"));

    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Löschen"));
    expect(await screen.findByText("Datei als root löschen?")).toBeVisible();
    vi.mocked(sftpDelete).mockResolvedValue(undefined);
    fireEvent.click(screen.getAllByText("Löschen").at(-1)!);

    await waitFor(() => expect(sftpDelete).toHaveBeenCalledWith("session-1", "a.txt", "root"));
    await waitFor(() =>
      expect(vi.mocked(showToast).mock.calls.at(-1)?.[0]).toMatchObject({
        kind: "success",
        message: "„a.txt“ gelöscht (als root)",
      }),
    );
  });

  it("explains a missing sudo rule with the tailored sudoers line and an honest warning", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: false,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: {
        kind: "passwordRequired",
        sudoersLine: "deploy ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server",
        detail: null,
      },
    });
    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));

    expect(
      await screen.findByText("deploy ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server"),
    ).toBeVisible();
    expect(screen.getByText(/passwortlosen Dateizugriff als root/)).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("can elevate to another user (A4) and names that user everywhere", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "www-data",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: null,
    });
    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.change(screen.getByLabelText(/Ziel-Nutzer/), { target: { value: "www-data" } });
    fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));

    await waitFor(() => expect(sftpElevationEnable).toHaveBeenCalledWith("session-1", "www-data"));
    expect(await screen.findByRole("alert")).toHaveTextContent("www-data");
  });

  it("uses root when no target user is given", async () => {
    await enableElevation();
    expect(sftpElevationEnable).toHaveBeenCalledWith("session-1", "root");
  });

  it("a drag-and-drop upload after switching uses the elevated channel (no stale closure)", async () => {
    vi.mocked(sftpExists).mockResolvedValue(false);
    // Issue #5: resolve with the arguments of the first upload, so the test
    // waits on the upload event itself instead of polling a spy until a
    // deadline. An upload over the wrong channel fails at once, with content.
    const firstUpload = new Promise<unknown[]>((resolve) => {
      vi.mocked(sftpUpload).mockImplementation((...args) => {
        resolve(args);
        return Promise.resolve();
      });
    });
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: null,
    });
    // The mock keeps the last listener across tests; start from none, so the
    // listener seen below is this panel's own.
    dragDrop.handler = null;
    renderPanel();
    await screen.findByText(/a\.txt/);
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    const normalModeHandler = dragDrop.handler;
    fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));
    await screen.findByRole("alert");

    // Issue #5 / #29: the banner (`role="alert"`) is committed before React
    // runs passive effects. The drop listener is no longer re-registered on a
    // channel switch (it reads the current channel when the drop is handled),
    // so a drop right after the banner appears must already use the new
    // channel — no waiting for a re-registered listener.
    expect(dragDrop.handler).toBe(normalModeHandler);

    act(() => dragDrop.handler!({ payload: { type: "drop", paths: ["/local/x.conf"] } }));

    expect(await firstUpload).toEqual(["session-1", "/local/x.conf", "x.conf", "root"]);
    expect(sftpUpload).toHaveBeenCalledTimes(1);
  });

  // Issue #29: the drop listener registered before a switch may still be the
  // one that receives the event (React commits the new UI before passive
  // effects run). It must use the channel that is current when the drop is
  // handled, in both directions.
  it("a drop via the listener registered before switching elevated on uploads as root", async () => {
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: null,
    });
    dragDrop.handler = null;
    dragDrop.registrations = 0;
    renderPanel();
    await screen.findByText(/a\.txt/);
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    const handlerBeforeSwitch = dragDrop.handler!;

    fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));
    await screen.findByRole("alert");
    // Let every pending effect and listener registration settle, so a
    // re-registration (the old behaviour) would have happened by now.
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", ".", "root"));

    act(() => handlerBeforeSwitch({ payload: { type: "drop", paths: ["/local/x.conf"] } }));

    await waitFor(() => expect(sftpUpload).toHaveBeenCalledTimes(1));
    expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/x.conf", "x.conf", "root");
    expect(sftpExists).toHaveBeenCalledWith("session-1", "x.conf", "root");
    expect(dragDrop.registrations).toBe(1);
  });

  it("a drop via the listener registered in elevated mode uploads normally after switching off", async () => {
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    dragDrop.handler = null;
    dragDrop.registrations = 0;
    await enableElevation();
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", ".", "root"));
    const handlerWhileElevated = dragDrop.handler!;
    expect(handlerWhileElevated).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Erhöhte Rechte beenden" }));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", ".", null));

    act(() => handlerWhileElevated({ payload: { type: "drop", paths: ["/local/x.conf"] } }));

    await waitFor(() => expect(sftpUpload).toHaveBeenCalledTimes(1));
    expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/x.conf", "x.conf", null);
    expect(sftpExists).toHaveBeenCalledWith("session-1", "x.conf", null);
    expect(dragDrop.registrations).toBe(1);
  });

  it("a drop via the listener registered before navigating uploads into the new directory", async () => {
    const dirEntry: RemoteEntryDto = { ...entry, name: "logs", path: "logs", isDir: true };
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    dragDrop.handler = null;
    dragDrop.registrations = 0;
    renderPanel();
    const dirButton = await screen.findByText(/logs/);
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    const handlerBeforeNavigation = dragDrop.handler!;

    fireEvent.click(dirButton);
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", "logs", null));
    await waitFor(() => expect(screen.getByDisplayValue("logs")).toBeInTheDocument());

    act(() => handlerBeforeNavigation({ payload: { type: "drop", paths: ["/local/x.conf"] } }));

    await waitFor(() => expect(sftpUpload).toHaveBeenCalledTimes(1));
    expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/x.conf", "logs/x.conf", null);
    expect(dragDrop.registrations).toBe(1);
  });

  it("a hidden panel registers no drop listener and removes it when hidden", async () => {
    dragDrop.handler = null;
    dragDrop.registrations = 0;
    dragDrop.unlistens = 0;
    const { rerender } = render(
      <I18nextProvider i18n={testI18n}>
        <FileBrowserPanel sessionId="session-1" isVisible={false} />
      </I18nextProvider>,
    );
    await screen.findByText(/a\.txt/);
    expect(dragDrop.registrations).toBe(0);
    expect(dragDrop.handler).toBeNull();

    rerender(
      <I18nextProvider i18n={testI18n}>
        <FileBrowserPanel sessionId="session-1" isVisible={true} />
      </I18nextProvider>,
    );
    await waitFor(() => expect(dragDrop.registrations).toBe(1));
    // Let the registration promise resolve, so hiding unlistens directly.
    await act(async () => {});

    rerender(
      <I18nextProvider i18n={testI18n}>
        <FileBrowserPanel sessionId="session-1" isVisible={false} />
      </I18nextProvider>,
    );
    await waitFor(() => expect(dragDrop.unlistens).toBe(1));
    expect(dragDrop.registrations).toBe(1);
  });

  it("reports the elevated user upward so it stays visible when the browser is hidden", async () => {
    const onElevationChange = vi.fn();
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: null,
    });
    render(
      <I18nextProvider i18n={testI18n}>
        <FileBrowserPanel sessionId="session-1" isVisible={true} onElevationChange={onElevationChange} />
      </I18nextProvider>,
    );
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));

    await waitFor(() => expect(onElevationChange).toHaveBeenLastCalledWith("root"));
    fireEvent.click(screen.getByRole("button", { name: "Erhöhte Rechte beenden" }));
    await waitFor(() => expect(onElevationChange).toHaveBeenLastCalledWith(null));
  });

  it("drops the elevated mode when the connection goes away", async () => {
    let statusHandler: ((event: { sessionId: string; status: string; reason: null }) => void) | null =
      null;
    vi.mocked(onConnectionStatusChanged).mockImplementation((h) => {
      statusHandler = h as typeof statusHandler;
      return Promise.resolve(() => {});
    });
    await enableElevation();

    act(() => statusHandler!({ sessionId: "session-1", status: "disconnected", reason: null }));

    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("a file opened elevated is uploaded elevated even after the toggle is off — never silently as the normal user", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/a.txt",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpReadText).mockResolvedValue("alt");
    vi.mocked(sftpStat).mockResolvedValue({ ...fileEntry, modified: "2026-01-01T00:00:00Z" });
    vi.mocked(sftpUpload).mockRejectedValue({
      message: "Der erhöhte Modus ist nicht mehr aktiv",
      code: null,
    });
    await enableElevation();

    // ADR 0122, R7: "wurde lokal geändert" hängt an einem echten
    // Produktcode-Intervall (`POLL_INTERVAL_MS` in `useLocalEditSession`,
    // 2000ms) — ab hier Fake-Timer, damit der Poll-Tick steuerbar ist, statt
    // sich auf ein festes 4s-Timeout über zwei Ticks hinweg zu verlassen
    // (das unter Last reißen kann). Testing-Library
    // erkennt vitests Fake-Timer hier nicht (kein globales `jest`, s.
    // `@testing-library/dom`s `jestFakeTimersAreEnabled`) — `waitFor`/
    // `findBy*` würden also hängen bleiben; ab hier deshalb per
    // `advanceTimersByTimeAsync` flushen und synchron (`getBy*`) prüfen.
    vi.useFakeTimers();
    try {
      fireEvent.click(screen.getByRole("button", { name: "⋮" }));
      fireEvent.click(screen.getByText("Lokal öffnen…"));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(sftpOpenForEditing).toHaveBeenCalledWith("session-1", "a.txt", "root");
      expect(screen.getByText(/als root geöffnet/)).toBeVisible();

      // Modus ausschalten, danach die lokale Änderung hochladen.
      fireEvent.click(screen.getByRole("button", { name: "Erhöhte Rechte beenden" }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:09Z");
      await act(async () => {
        await vi.runOnlyPendingTimersAsync();
      });
      const changed = screen.getByText(/wurde lokal geändert/);
      fireEvent.click(within(changed.parentElement!).getByRole("button", { name: "Hochladen" }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      const dialog = screen
        .getByText(/Lokale Änderungen als root hochladen\?/)
        .closest(".fixed") as HTMLElement;
      fireEvent.click(within(dialog).getByRole("button", { name: "Hochladen" }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      expect(sftpUpload).toHaveBeenCalledWith("session-1", "/tmp/edit/a.txt", "a.txt", "root");
      expect(sftpUpload).not.toHaveBeenCalledWith("session-1", "/tmp/edit/a.txt", "a.txt", null);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("FileBrowserPanel upload failure summary (Spec 0067, B2)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpList).mockResolvedValue([{ ...entry, name: "a.txt", path: "a.txt" }]);
  });

  it("several failed uploads produce one error toast, not one per file", async () => {
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockRejectedValue({ message: "Permission denied", code: null });
    vi.mocked(pickUploadFiles).mockResolvedValue(["/local/x", "/local/y", "/local/z"]);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Hochladen"));

    await waitFor(() =>
      expect(vi.mocked(showToast).mock.calls.at(-1)?.[0]).toMatchObject({
        kind: "error",
        message: expect.stringContaining("3 Dateien konnten nicht hochgeladen werden"),
      }),
    );
    expect(showToast).toHaveBeenCalledTimes(1);
  });
});

// Issue #89: a drop uploads only the paths the backend captured from the
// native drop event and granted to this session. The webview's own event
// payload is not used as a path source.
describe("FileBrowserPanel drop uses backend-granted paths (Issue #89)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpList).mockResolvedValue([{ ...entry, name: "a.txt", path: "a.txt" }]);
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    dragDrop.handler = null;
  });

  it("uploads the claimed paths, not the paths in the event payload", async () => {
    vi.mocked(claimDroppedPaths).mockResolvedValueOnce(["/native/real.txt"]);

    renderPanel();
    await screen.findByText(/a\.txt/);
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    act(() =>
      dragDrop.handler!({ payload: { type: "drop", paths: ["/home/user/.ssh/id_ed25519"] } }),
    );

    await waitFor(() => expect(sftpUpload).toHaveBeenCalledTimes(1));
    expect(claimDroppedPaths).toHaveBeenCalledWith("session-1");
    expect(sftpUpload).toHaveBeenCalledWith("session-1", "/native/real.txt", "real.txt", null);
    expect(sftpUpload).not.toHaveBeenCalledWith(
      "session-1",
      "/home/user/.ssh/id_ed25519",
      expect.anything(),
      expect.anything(),
    );
  });

  it("a claim with nothing to hand out uploads nothing", async () => {
    vi.mocked(claimDroppedPaths).mockResolvedValueOnce([]);

    renderPanel();
    await screen.findByText(/a\.txt/);
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    act(() => dragDrop.handler!({ payload: { type: "drop", paths: ["/local/x.txt"] } }));

    await waitFor(() => expect(claimDroppedPaths).toHaveBeenCalledTimes(1));
    await act(async () => {});
    expect(sftpExists).not.toHaveBeenCalled();
    expect(sftpUpload).not.toHaveBeenCalled();
  });

  // ADR 0113: folder upload is not added here. A dropped folder is forwarded
  // exactly like a file (and fails in the backend as before).
  it("a dropped folder is forwarded the same way as a file", async () => {
    renderPanel();
    await screen.findByText(/a\.txt/);
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    act(() => dragDrop.handler!({ payload: { type: "drop", paths: ["/local/project"] } }));

    await waitFor(() => expect(sftpUpload).toHaveBeenCalledTimes(1));
    expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/project", "project", null);
  });
});

// Issue #91: every label and dialog of the file browser follows the UI
// language. These tests render the panel with an `en` clone of the test
// i18n instance and walk through the toolbar, table, entry menu and each
// dialog; the German wording is checked separately below.
describe("FileBrowserPanel UI language (issue #91)", () => {
  const enI18n = testI18n.cloneInstance({ lng: "en" });
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };
  const dirEntry: RemoteEntryDto = {
    ...entry,
    name: "logs",
    path: "logs",
    isDir: true,
    permissionsOctal: 0o755,
    permissions: "rwxr-xr-x",
  };

  /** Words of the former hard-coded German texts. None of them may show up
   * as text, `title` or `aria-label` while the UI is English. */
  const GERMAN_WORDS = [
    "Abbrechen", "Aktualisieren", "Ausführen", "Ausschneiden", "Bearbeitung",
    "Besitzer", "Datei", "Einfügen", "Eigenschaften", "Enthält", "Ermittle",
    "existiert", "Geändert", "Größe", "Gruppe", "Herunterladen", "Hochladen",
    "kopieren", "Lädt", "Lesen", "Löschen", "Numerisch", "Ordner", "Pfad",
    "Rechte", "Rekursiv", "Schließen", "Schreiben", "Später", "Startverzeichnis",
    "Übergeordnetes", "Übernehmen", "Überschreiben", "Umbenennen", "Verzeichnis",
    "wird", "würde",
  ];

  function expectNoGerman() {
    const texts = [document.body.textContent ?? ""];
    for (const el of document.body.querySelectorAll("[title], [aria-label]")) {
      texts.push(el.getAttribute("title") ?? "", el.getAttribute("aria-label") ?? "");
    }
    const all = texts.join("\n");
    const found = GERMAN_WORDS.filter((word) => new RegExp(`(^|[^\\p{L}])${word}`, "u").test(all));
    expect(found, `German text visible: ${found.join(", ")}`).toEqual([]);
    expect(all).not.toMatch(/[äöüÄÖÜß„]/);
  }

  function renderEn(i18n = enI18n) {
    return render(
      <I18nextProvider i18n={i18n}>
        <FileBrowserPanel sessionId="session-1" isVisible={true} />
      </I18nextProvider>,
    );
  }

  function openMenu() {
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
  }

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    dragDrop.handler = null;
  });

  it("toolbar, table header, entry menu and drop hint are English", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);

    renderEn();
    await screen.findByText(/a\.txt/);

    expect(screen.getByTitle("Go to start directory")).toBeInTheDocument();
    expect(screen.getByTitle("Parent directory")).toBeInTheDocument();
    expect(screen.getByText("+ Folder")).toBeVisible();
    expect(screen.getByText("Upload")).toBeVisible();
    for (const header of ["Name", "Size", "Permissions", "Modified"]) {
      expect(screen.getByRole("columnheader", { name: header })).toBeInTheDocument();
    }

    openMenu();
    for (const item of [
      "Download",
      "Download to…",
      "Copy file content",
      "Open locally…",
      "Copy path",
      "Properties",
      "Refresh",
      "Edit permissions…",
      "Rename",
      "Cut",
      "Delete",
    ]) {
      expect(screen.getByRole("button", { name: item })).toBeVisible();
    }
    expectNoGerman();

    // Cut + paste buttons.
    fireEvent.click(screen.getByRole("button", { name: "Cut" }));
    expect(screen.getByRole("button", { name: "Paste" })).toHaveAttribute("title", "Move a.txt here");
    expect(screen.getByTitle("Cancel cut")).toBeInTheDocument();

    // Drop hint while dragging files over the panel.
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    act(() => dragDrop.handler!({ payload: { type: "over" } }));
    expect(screen.getByText(/^Drop here to upload to /)).toBeVisible();
    expectNoGerman();
  });

  it("loading and empty directory texts are English", async () => {
    let resolveList: (entries: RemoteEntryDto[]) => void = () => {};
    vi.mocked(sftpList).mockReturnValue(
      new Promise((resolve) => {
        resolveList = resolve;
      }),
    );

    renderEn();
    expect(await screen.findByText("Loading…")).toBeVisible();
    await act(async () => resolveList([]));
    expect(await screen.findByText("(empty directory)")).toBeVisible();
    expectNoGerman();
  });

  it("transfer progress lines are English", async () => {
    let started: ((event: SftpTransferStartedEvent) => void) | null = null;
    vi.mocked(onSftpTransferStarted).mockImplementation((handler) => {
      started = handler;
      return Promise.resolve(() => {});
    });
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);

    renderEn();
    await screen.findByText(/a\.txt/);
    await waitFor(() => expect(started).not.toBeNull());
    act(() => {
      started!({ sessionId: "session-1", transferId: "t1", kind: "upload", fileName: "up.bin", totalBytes: null });
      started!({ sessionId: "session-1", transferId: "t2", kind: "download", fileName: "down.bin", totalBytes: null });
    });

    expect(screen.getByText("Uploading: up.bin")).toBeVisible();
    expect(screen.getByText("Downloading: down.bin")).toBeVisible();
    expectNoGerman();
  });

  it("the native upload dialog gets an English title", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(pickUploadFiles).mockResolvedValue(null);

    renderEn();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Upload"));

    await waitFor(() => expect(pickUploadFiles).toHaveBeenCalledWith("session-1", "Upload file(s)"));
  });

  // Issue #153: the native folder dialog of "Download to…" on a directory
  // used to have a hard-coded German title.
  it("the native folder-download dialog gets an English title", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(sftpDownloadDir).mockResolvedValue(null);

    renderEn();
    await screen.findByText(/logs/);
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Download to…" }));

    await waitFor(() =>
      expect(sftpDownloadDir).toHaveBeenCalledWith("session-1", "logs", "Choose destination folder", null),
    );
    expect(sftpDownload).not.toHaveBeenCalled();
  });

  it("properties dialog is English", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);

    renderEn();
    await screen.findByText(/a\.txt/);
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Properties" }));

    expect(await screen.findByRole("heading", { name: "Properties" })).toBeVisible();
    for (const label of [
      "Path",
      "Type",
      "File",
      "Permissions (symbolic)",
      "Permissions (numeric)",
      "Owner",
      "Group",
      "Close",
    ]) {
      expect(screen.getByText(label)).toBeVisible();
    }
    expectNoGerman();
  });

  it("chmod dialog, including the recursive warning, is English", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);

    renderEn();
    await screen.findByText(/logs/);
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Edit permissions…" }));

    expect(await screen.findByRole("heading", { name: "Edit permissions" })).toBeVisible();
    for (const header of ["Read", "Write", "Execute"]) {
      expect(screen.getByRole("columnheader", { name: header })).toBeInTheDocument();
    }
    expect(screen.getByText("Numeric")).toBeVisible();
    expect(screen.getByText("Recursive")).toBeVisible();
    expect(screen.getByText(/changes the permissions of ALL files and subfolders/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Apply" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible();
    expectNoGerman();
  });

  it("rename and new-folder prompts are English", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);

    renderEn();
    await screen.findByText(/a\.txt/);
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    expect(await screen.findByRole("heading", { name: "Rename" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Apply" })).toBeVisible();
    expectNoGerman();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    fireEvent.click(screen.getByText("+ Folder"));
    expect(await screen.findByRole("heading", { name: "New folder" })).toBeVisible();
    expectNoGerman();
  });

  it("delete dialog for a folder is English, with plural counts", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(sftpDeletePreview).mockResolvedValue({ fileCount: 3, dirCount: 1 });

    renderEn();
    await screen.findByText(/logs/);
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    const heading = await screen.findByRole("heading", { name: "Delete folder?" });
    const body = heading.nextElementSibling as HTMLElement;
    await waitFor(() =>
      expect(body.textContent).toBe(
        "logs will be deleted from the server permanently. Contains 3 files in 1 folder (including this one) — all of them will be deleted.",
      ),
    );
    // The counts stay emphasised.
    expect(within(body).getByText("3").tagName).toBe("STRONG");
    expect(within(body).getByText("1").tagName).toBe("STRONG");
    expectNoGerman();
    expect(sftpDelete).not.toHaveBeenCalled();
  });

  it("delete dialog for a file is English", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);

    renderEn();
    await screen.findByText(/a\.txt/);
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    expect(await screen.findByRole("heading", { name: "Delete file?" })).toBeVisible();
    expect(screen.getByText("will be deleted from the server permanently.", { exact: false })).toBeVisible();
    expectNoGerman();
  });

  it("move collision dialog is English", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(sftpExists).mockResolvedValue(true);

    renderEn();
    await screen.findByText(/a\.txt/);
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    fireEvent.change(screen.getByDisplayValue("a.txt"), { target: { value: "b.txt" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    expect(await screen.findByRole("heading", { name: "Target already exists" })).toBeVisible();
    expect(screen.getByText(/already exists and would be overwritten\./)).toBeVisible();
    expect(screen.getByRole("button", { name: "Overwrite" })).toBeVisible();
    expectNoGerman();
  });

  it("upload conflict dialog without a text diff is English", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(sftpExists).mockResolvedValue(true);
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: null, size: 2048 });
    vi.mocked(sftpReadText).mockRejectedValue("binary");
    vi.mocked(pickUploadFiles).mockResolvedValue(["/local/a.txt"]);

    renderEn();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Upload"));

    expect(await screen.findByRole("heading", { name: "Overwrite file?" })).toBeVisible();
    expect(screen.getByText(/already exists on the server\./)).toBeVisible();
    expect(
      screen.getByText(/^No text diff possible \(binary file or too large\)\. Current size: .+, new size: .+\.$/),
    ).toBeVisible();
    expectNoGerman();
  });

  it("local edit status bar and upload offer are English", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/a.txt",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    // First call: baseline after opening; every later poll sees a change.
    vi.mocked(localFileMtime)
      .mockResolvedValueOnce("2026-01-01T00:00:01Z")
      .mockResolvedValue("2026-01-01T00:00:09Z");
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: null, size: 10 });
    vi.mocked(sftpStat).mockResolvedValue({ ...fileEntry, modified: "2026-01-01T00:00:05Z" });
    vi.mocked(sftpReadText).mockRejectedValue("binary");

    renderEn();
    await screen.findByText(/a\.txt/);
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Open locally…" }));

    expect(await screen.findByText("Editing “a.txt” locally…")).toBeVisible();
    expect(screen.getByRole("button", { name: "Stop editing" })).toBeVisible();
    expectNoGerman();

    expect(
      await screen.findByText("“a.txt” was changed locally. Upload it to the server?", undefined, {
        timeout: 4000,
      }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Later" })).toBeVisible();
    expectNoGerman();

    // The second "Upload" button is the one in the status bar.
    fireEvent.click(screen.getAllByRole("button", { name: "Upload" })[1]);
    expect(await screen.findByRole("heading", { name: "Upload local changes?" })).toBeVisible();
    expect(
      screen.getByText("The remote file was changed since the download — uploading would overwrite that other change."),
    ).toBeVisible();
    expect(screen.getByText(/^No text diff possible \(binary file or too large\)\. New size: .+\.$/)).toBeVisible();
    expectNoGerman();
  });

  it("German wording stays as before", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(sftpDeletePreview).mockResolvedValue({ fileCount: 1, dirCount: 1 });

    renderEn(testI18n);
    await screen.findByText(/logs/);
    expect(screen.getByTitle("Übergeordnetes Verzeichnis")).toBeInTheDocument();
    openMenu();
    fireEvent.click(screen.getByRole("button", { name: "Löschen" }));

    const heading = await screen.findByRole("heading", { name: "Ordner löschen?" });
    const body = heading.nextElementSibling as HTMLElement;
    await waitFor(() =>
      expect(body.textContent).toBe(
        "logs wird unwiderruflich vom Server gelöscht. Enthält 1 Datei(en) in 1 Ordner(n) (inkl. diesem) — alle werden mitgelöscht.",
      ),
    );
  });
});
