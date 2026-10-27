// Spec 0053, Teil 1 ("Testbarkeit"): "Spaltenbreite ziehen -> Breite ändert
// sich, Mindestbreite wird nicht unterschritten, Wert überlebt Neustart."
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
  claimClipboardFiles,
  claimDroppedPaths,
  clipboardHasFiles,
  clipboardWriteText,
  closeEditSession,
  localFileMtime,
  pickUploadFiles,
  pickUploadFolder,
  sftpUploadFolder,
  sftpUploadFolderPreview,
  readLocalTextPreview,
  sftpChmod,
  sftpChown,
  sftpDelete,
  sftpDeletePreview,
  sftpDownload,
  sftpDownloadDefault,
  sftpDownloadDir,
  sftpCancelTransfer,
  sftpElevationDisable,
  sftpElevationEnable,
  sftpExists,
  sftpCreateFile,
  sftpMkdir,
  sftpList,
  sftpOpenForEditing,
  sftpCancelCopyContent,
  sftpCopyContent,
  sftpCopyFile,
  sftpReadText,
  sftpRename,
  sftpStat,
  sftpUpload,
  sessionIsRoot,
} from "../api";
import {
  onConnectionStatusChanged,
  onSftpTransferFinished,
  onSftpTransferProgress,
  onSftpTransferStarted,
} from "../events";
import {
  loadFileManagerColumnWidths,
  loadFileManagerSort,
  saveFileManagerColumnWidths,
  loadHiddenFileColumns,
  saveHiddenFileColumns,
  saveFileManagerSort,
} from "../layoutSettings";
import { testI18n } from "../testI18n";
import { showToast } from "../toastBus";
import type { RemoteEntryDto, SftpTransferStartedEvent } from "../types";
import { FileBrowserPanel } from "./FileBrowserPanel";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err
      ? String((err as { message: unknown }).message)
      : String(err),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err
      ? String((err as { code: unknown }).code)
      : null,
  sftpCopyContent: vi.fn(),
  sftpCopyFile: vi.fn(),
  sftpCancelCopyContent: vi.fn(() => Promise.resolve()),
  sftpList: vi.fn(),
  sftpDelete: vi.fn(),
  sftpDeletePreview: vi.fn(),
  sftpDownload: vi.fn(),
  sftpDownloadDefault: vi.fn(),
  sftpDownloadDir: vi.fn(),
  sftpElevationDisable: vi.fn(() => Promise.resolve({ disabled: true, runningOperations: [] })),
  sftpCancelTransfer: vi.fn(() => Promise.resolve(true)),
  sftpElevationEnable: vi.fn(),
  sftpElevationStatus: vi.fn(() => Promise.resolve(null)),
  sftpMkdir: vi.fn(),
  sftpCreateFile: vi.fn(),
  sftpReadText: vi.fn(),
  sftpRename: vi.fn(),
  sftpUpload: vi.fn(),
  sftpExists: vi.fn(),
  sftpChmod: vi.fn(),
  sftpChown: vi.fn(),
  readLocalTextPreview: vi.fn(),
  // Issue #89: the backend hands out the paths of the native drop it
  // captured — here the paths of the last simulated drop event.
  claimDroppedPaths: vi.fn(() => Promise.resolve(nativeDrop.paths)),
  pickUploadFiles: vi.fn(),
  clipboardHasFiles: vi.fn(() => Promise.resolve(false)),
  clipboardWriteText: vi.fn(() => Promise.resolve()),
  claimClipboardFiles: vi.fn(),
  pickUploadFolder: vi.fn(),
  sftpLocalPathsAreFolders: vi.fn((_s: string, paths: string[]) =>
    Promise.resolve(paths.map(() => false)),
  ),
  sftpUploadFolder: vi.fn(),
  sftpUploadFolderPreview: vi.fn(),
  sftpStat: vi.fn(),
  sftpOpenForEditing: vi.fn(),
  localFileMtime: vi.fn(),
  closeEditSession: vi.fn(),
  sessionIsRoot: vi.fn(() => Promise.resolve(false)),
}));

vi.mock("../events", () => ({
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferStarted: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferFinished: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferProgress: vi.fn(() => Promise.resolve(() => {})),
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
  handler: null as null | ((event: { payload: { type: string; paths?: string[]; position?: { x: number; y: number } } }) => void),
  // Issue #29: how often a listener was registered / removed, so tests can
  // assert that a channel switch or a navigation does not re-register it.
  registrations: 0,
  unlistens: 0,
}));

// Position-based drop routing: jsdom has no layout, so the panel (the element
// carrying `data-elevated`) gets a 0..100 CSS-pixel rect and an offsetParent.
// Positions are physical pixels (CSS pixels * devicePixelRatio).
const IN = { x: 10, y: 10 };
const OUT = { x: 500, y: 500 };
Object.defineProperty(HTMLElement.prototype, "offsetParent", {
  configurable: true,
  get() {
    return document.body;
  },
});
HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
  const panel = this.hasAttribute("data-elevated");
  return {
    left: 0,
    top: 0,
    right: panel ? 100 : 0,
    bottom: panel ? 100 : 0,
    width: 0,
    height: 0,
    x: 0,
    y: 0,
    toJSON() {},
  } as DOMRect;
};

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((h) => {
      dragDrop.handler = (event: { payload: { type: string; paths?: string[]; position?: { x: number; y: number } } }) => {
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
  loadHiddenFileColumns: vi.fn(() => Promise.resolve([])),
  saveHiddenFileColumns: vi.fn(() => Promise.resolve()),
  loadFileManagerSort: vi.fn(() => Promise.resolve(null)),
  saveFileManagerSort: vi.fn(() => Promise.resolve()),
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

  it("header cells are sticky with an opaque background and the table has no collapsed borders (#650)", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);
    const ths = container.querySelectorAll("thead th");
    expect(ths.length).toBe(6);
    ths.forEach((th) => {
      expect(th).toHaveClass("sticky", "top-0", "bg-surface");
    });
    const table = container.querySelector("table");
    expect(table).toHaveClass("border-separate", "border-spacing-0");
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
    // 600px Container; andere Spalten (Rechte 90 + Besitzer 130 + Geändert
    // 150 + Aktionen 36) + Name-Minimum (120) = 526px stehen fest, für
    // "Größe" bleiben maximal 600 - 526 = 74px.
    stubContainerWidth(container, 600);

    const handle = screen.getByTestId("column-resize-size");
    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerMove(handle, { clientX: 600, buttons: 1 }); // weit über das Maximum hinaus
    fireEvent.pointerUp(handle, { clientX: 600 });

    await waitFor(() => {
      const sizeCol = container.querySelectorAll("colgroup col")[1] as HTMLElement;
      expect(sizeCol.style.width).toBe("74px");
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

describe("FileBrowserPanel Owner column and column visibility (issue #653)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(loadHiddenFileColumns).mockResolvedValue([]);
  });

  afterEach(() => {
    vi.mocked(loadHiddenFileColumns).mockResolvedValue([]);
  });

  it("shows uid:gid when names are missing and user:group when present", async () => {
    vi.mocked(sftpList).mockResolvedValue([
      entry,
      { ...entry, name: "b.txt", path: "b.txt", owner: "alice", group: "staff" },
    ]);
    renderPanel();
    await screen.findByText(/readme\.md/);
    expect(screen.getByText("1000:1000")).toBeTruthy();
    expect(screen.getByText("alice:staff")).toBeTruthy();
  });

  it("hides and shows columns via the header context menu, Name is not offered, and persists", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry]);
    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);

    fireEvent.contextMenu(container.querySelector("thead tr") as HTMLElement, { clientX: 10, clientY: 10 });
    const menu = await screen.findByRole("menu");
    const items = within(menu).getAllByRole("menuitemcheckbox");
    expect(items).toHaveLength(4);
    expect(within(menu).queryByRole("menuitemcheckbox", { name: /^Name$/ })).toBeNull();

    fireEvent.click(items[2]);
    await waitFor(() => expect(container.querySelectorAll("thead th").length).toBe(5));
    expect(screen.queryByText("1000:1000")).toBeNull();
    expect(saveHiddenFileColumns).toHaveBeenCalledWith(["owner"]);

    fireEvent.click(within(screen.getByRole("menu")).getAllByRole("menuitemcheckbox")[2]);
    await waitFor(() => expect(container.querySelectorAll("thead th").length).toBe(6));
    expect(screen.getByText("1000:1000")).toBeTruthy();
    expect(saveHiddenFileColumns).toHaveBeenLastCalledWith([]);
  });

  it("applies the stored hidden columns on mount", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadHiddenFileColumns).mockResolvedValue(["size", "permissions", "owner", "modified"]);
    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);
    await waitFor(() => expect(container.querySelectorAll("thead th").length).toBe(2));
    expect(container.querySelectorAll("colgroup col").length).toBe(2);
  });

  it("column resizing still works while another column is hidden", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry]);
    vi.mocked(loadHiddenFileColumns).mockResolvedValue(["permissions"]);
    const { container } = renderPanel();
    await screen.findByText(/readme\.md/);
    stubContainerWidth(container, 1000);
    await waitFor(() => expect(container.querySelectorAll("colgroup col").length).toBe(5));

    const handle = screen.getByTestId("column-resize-owner");
    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerMove(handle, { clientX: 130, buttons: 1 });
    fireEvent.pointerUp(handle, { clientX: 130 });

    await waitFor(() => {
      // order: name, size, owner, modified, actions; default owner 130 + 30
      expect((container.querySelectorAll("colgroup col")[2] as HTMLElement).style.width).toBe("160px");
    });
    expect(screen.queryByTestId("column-resize-permissions")).toBeNull();
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

    const triggers = screen.getAllByRole("button", { name: /^(Aktionen|Actions): / });
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

    const triggers = screen.getAllByRole("button", { name: /^(Aktionen|Actions): / });
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

  it("marks only the row whose menu is open as active (issue #651)", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry, dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    const row = (await screen.findByText(/a\.txt/)).closest("tr")!;
    const other = screen.getByText(/logs/).closest("tr")!;
    expect(row).not.toHaveAttribute("data-active");

    // hover: stronger background and underlined name
    expect(row.className).toContain("hover:bg-border/60");
    expect(row.className).toContain("group");
    expect(screen.getByText(/a\.txt/).className).toContain("group-hover:underline");

    // right-click
    fireEvent.contextMenu(row);
    expect(row).toHaveAttribute("data-active", "true");
    expect(other).not.toHaveAttribute("data-active");
    // Escape closes
    fireEvent.keyDown(document, { key: "Escape" });
    expect(row).not.toHaveAttribute("data-active");

    // three dots on the other row: only that row is active
    fireEvent.contextMenu(row);
    fireEvent.click(other.querySelector("[data-menu-trigger]")!);
    expect(other).toHaveAttribute("data-active", "true");
    expect(row).not.toHaveAttribute("data-active");
    // outside click closes
    fireEvent.click(document.body);
    expect(other).not.toHaveAttribute("data-active");
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Herunterladen"));

    expect(sftpDownloadDefault).toHaveBeenCalledWith("session-1", "a.txt", null);
  });

  it("'Herunterladen nach…' uses the folder dialog for directories, file dialog for files", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpDownloadDir).mockResolvedValue(null);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Herunterladen nach…"));

    expect(sftpDownloadDir).toHaveBeenCalledWith("session-1", "logs", "Zielordner wählen", null);
    expect(sftpDownload).not.toHaveBeenCalled();
  });

  it("copies the path to the clipboard", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    vi.mocked(sftpCopyContent).mockResolvedValue("hallo welt");

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));

    await waitFor(() =>
      expect(clipboardWriteText).toHaveBeenCalledWith("hallo welt"),
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

  it("offers Copy file for files only and confirms a successful copy", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpCopyFile).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Datei kopieren"));

    await waitFor(() => expect(sftpCopyFile).toHaveBeenCalled());
    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ kind: "success", message: expect.stringContaining("a.txt") }),
      ),
    );
  });

  it("shows nothing for a cancelled Copy file and the translated message for a failure", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpCopyFile).mockRejectedValueOnce({ code: "SFTP_COPY_CANCELLED", message: "x" });

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Datei kopieren"));
    await waitFor(() => expect(sftpCopyFile).toHaveBeenCalledTimes(1));
    expect(showToast).not.toHaveBeenCalledWith(expect.objectContaining({ kind: "error" }));

    vi.mocked(sftpCopyFile).mockRejectedValueOnce({
      code: "CLIPBOARD_FILE_FAILED",
      message: "raw backend text",
    });
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Datei kopieren"));
    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ kind: "error", message: expect.stringContaining("Zwischenablage") }),
      ),
    );
    expect(vi.mocked(showToast).mock.calls.at(-1)?.[0].message).not.toContain("raw backend text");
  });

  it("copies a 10 MB file completely to the clipboard", async () => {
    const big = "x".repeat(10 * 1024 * 1024);
    vi.mocked(sftpList).mockResolvedValue([{ ...fileEntry, size: big.length }]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpCopyContent).mockResolvedValue(big);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));

    await waitFor(() => expect(clipboardWriteText).toHaveBeenCalledWith(big));
  });

  it("shows the translated message for a file over 50 MB, not the backend text", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpCopyContent).mockRejectedValue({
      code: "SFTP_CONTENT_TOO_LARGE",
      message: "raw backend text",
    });

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "error",
          message: expect.stringContaining("50 MB"),
        }),
      ),
    );
    expect(vi.mocked(showToast).mock.calls.at(-1)?.[0].message).not.toContain("raw backend text");
  });

  it("shows progress and a cancel action for a 6 MB file; cancelling copies nothing", async () => {
    const sixMb = 6 * 1024 * 1024;
    vi.mocked(sftpList).mockResolvedValue([{ ...fileEntry, size: sixMb }]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    let rejectCopy: (reason: unknown) => void = () => {};
    vi.mocked(sftpCopyContent).mockImplementation(
      (_s, _p, _u, _id, onProgress) =>
        new Promise<string>((_resolve, reject) => {
          rejectCopy = reject;
          onProgress({ done: 2 * 1024 * 1024, total: sixMb });
        }),
    );
    vi.mocked(showToast).mockClear();
    vi.mocked(clipboardWriteText).mockClear();

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));

    const status = await screen.findByRole("status");
    expect(status.textContent).toContain("2.0");
    expect(status.textContent).toContain("6.0");
    fireEvent.click(within(status).getByRole("button", { name: "Abbrechen" }));
    expect(sftpCancelCopyContent).toHaveBeenCalledTimes(1);

    rejectCopy({ code: "SFTP_COPY_CANCELLED", message: "cancelled" });
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(clipboardWriteText).not.toHaveBeenCalled();
    expect(showToast).not.toHaveBeenCalled();
  });

  it("shows no progress for a small file", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpCopyContent).mockImplementation(() => new Promise<string>(() => {}));

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));
    await waitFor(() => expect(sftpCopyContent).toHaveBeenCalled());
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows a toast with the backend error when the file is not text", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpCopyContent).mockRejectedValue("Datei ist keine Textdatei (kein gültiges UTF-8)");

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));

    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "error",
          message: expect.stringContaining("Datei ist keine Textdatei (kein gültiges UTF-8)"),
        }),
      ),
    );
    expect(clipboardWriteText).not.toHaveBeenCalled();
  });

  it("shows the properties dialog with numeric and symbolic permissions", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Eigenschaften"));

    expect(await screen.findByText("Eigenschaften")).toBeVisible();
    // "rw-r--r--" steht sowohl in der Tabellenspalte als auch im Dialog —
    // die numerische Darstellung ("644") ist eindeutig nur im Dialog.
    expect(screen.getAllByText("rw-r--r--").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("0644")).toBeVisible();
  });

  it("'Aktualisieren' reloads the current directory", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    expect(await screen.findByText("Rechte bearbeiten")).toBeVisible();
    // fileEntry ist 644 (rw-r--r--) — "Besitzer Schreiben" (0o200) abwählen.
    fireEvent.click(screen.getByLabelText("Besitzer: Schreiben"));
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "a.txt", 0o444, false, false, null),
    );
  });

  it("recursive chmod dialog shows the folder/file summary and sends special-bit changes (issue #624)", async () => {
    vi.mocked(sftpList).mockResolvedValue([{ ...dirEntry, permissionsOctal: 0o2755 }]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(3);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));
    await screen.findByDisplayValue("2755");

    expect(screen.queryByText(/^Ordner: /)).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(/^Rekursiv/));
    expect(screen.getByText("Ordner: 755, Dateien: 755")).toBeVisible();

    // 644 for the rwx part: folders get 755, files 644.
    fireEvent.change(screen.getByDisplayValue("2755"), { target: { value: "2644" } });
    expect(screen.getByText("Ordner: 755, Dateien: 644")).toBeVisible();
    fireEvent.click(screen.getByText("Übernehmen"));
    // Special bits untouched by the user: not applied explicitly.
    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "logs", 0o2644, true, false, null),
    );

    // Ticking setgid explicitly reports the change.
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));
    await screen.findByDisplayValue("2755");
    fireEvent.click(screen.getByLabelText("setuid-Bit"));
    fireEvent.click(screen.getByText("Übernehmen"));
    await waitFor(() =>
      expect(sftpChmod).toHaveBeenLastCalledWith("session-1", "logs", 0o6755, false, true, null),
    );
  });

  it("shows a permission hint for an empty folder without x (issue #624)", async () => {
    const locked: RemoteEntryDto = { ...dirEntry, permissionsOctal: 0o2644, permissions: "drw-r-Sr--" };
    vi.mocked(sftpList).mockImplementation(async (_s, p) => (p === "logs" ? [] : [locked]));
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    fireEvent.click(await screen.findByText(/logs/));

    expect(
      await screen.findByText("Keine Berechtigung, diesen Ordner zu öffnen (x fehlt)"),
    ).toBeVisible();
  });

  it("names the otherwise empty header cells of the file table and chmod matrix (issue #112)", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});

    renderPanel();
    await screen.findByText(/a\.txt/);
    expect(screen.getByRole("columnheader", { name: "Aktionen" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));
    expect(await screen.findByRole("columnheader", { name: "Gilt für" })).toBeInTheDocument();
  });

  it("chmod dialog's numeric input overrides the checkbox matrix", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(1);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    const numeric = await screen.findByDisplayValue("0644");
    fireEvent.change(numeric, { target: { value: "0700" } });
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "a.txt", 0o700, false, false, null),
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    expect(await screen.findByDisplayValue("4755")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "suid-bin", 0o4755, false, false, null),
    );
  });

  it("chmod dialog shows a recursive checkbox only for directories", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(1);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    const recursiveLabel = await screen.findByText(/Rekursiv/);
    fireEvent.click(recursiveLabel.closest("label")!.querySelector("input")!);
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "logs", 0o755, true, false, null),
    );
  });

  it("delete confirmation shows a file/dir count preview for folders", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpDeletePreview).mockResolvedValue({ fileCount: 3, dirCount: 2 });

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    vi.mocked(sftpStat).mockResolvedValue({ ...fileEntry, isDir: false });
    vi.mocked(sftpRename).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Umbenennen"));

    const input = screen.getByDisplayValue("a.txt");
    fireEvent.change(input, { target: { value: "b.txt" } });
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() => expect(sftpExists).toHaveBeenCalledWith("session-1", "b.txt", null));
    expect(await screen.findByText("Ziel existiert bereits")).toBeVisible();
    expect(sftpRename).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Überschreiben"));
    await waitFor(() => expect(sftpRename).toHaveBeenCalledWith("session-1", "a.txt", "b.txt", null, true));
  });

  it("rename onto an existing folder offers no overwrite and says it cannot be replaced", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(true);
    vi.mocked(sftpStat).mockResolvedValue({ ...fileEntry, name: "b.txt", path: "b.txt", isDir: true });
    vi.mocked(sftpRename).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Umbenennen"));
    fireEvent.change(screen.getByDisplayValue("a.txt"), { target: { value: "b.txt" } });
    fireEvent.click(screen.getByText("Übernehmen"));

    expect(await screen.findByText(/als Ordner und kann nicht ersetzt werden/)).toBeVisible();
    expect(screen.queryByText("Überschreiben")).not.toBeInTheDocument();
    expect(sftpRename).not.toHaveBeenCalled();
  });

  it("rename proceeds directly when there is no collision", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpRename).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    // Ordner stehen oben (Spec 0053, Teil 3) — Zeile für a.txt gezielt wählen.
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): a\.txt/ }));
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

  // Issue #128: folder upload.
  const folderPreview = (overwrites: string[] = []) => ({
    folderName: "proj",
    fileCount: 2,
    folderCount: 1,
    overwrites,
    skipped: [],
  });
  const folderSummary = {
    folderName: "proj",
    filesUploaded: 2,
    foldersCreated: 2,
    skipped: [],
    failed: [],
    notAttempted: 0,
  };

  it("'Ordner hochladen' picks a folder in the backend and uploads it without a dialog when nothing is overwritten", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(pickUploadFolder).mockResolvedValue("/local/proj");
    vi.mocked(sftpUploadFolderPreview).mockResolvedValue(folderPreview());
    vi.mocked(sftpUploadFolder).mockResolvedValue(folderSummary);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Ordner hochladen"));

    await waitFor(() =>
      expect(sftpUploadFolder).toHaveBeenCalledWith("session-1", "/local/proj", ".", [], null),
    );
    expect(pickUploadFolder).toHaveBeenCalledWith("session-1", "Ordner hochladen");
    expect(screen.queryByText("Dateien im Ordner überschreiben?")).not.toBeInTheDocument();
  });

  it("a folder that would overwrite files asks once, listing them, and uploads nothing before the confirmation", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(pickUploadFolder).mockResolvedValue("/local/proj");
    vi.mocked(sftpUploadFolderPreview).mockResolvedValue(folderPreview(["proj/a.txt", "proj/b.txt"]));
    vi.mocked(sftpUploadFolder).mockResolvedValue(folderSummary);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Ordner hochladen"));

    expect(await screen.findByText("Dateien im Ordner überschreiben?")).toBeVisible();
    expect(screen.getByText("proj/b.txt")).toBeVisible();
    expect(sftpUploadFolder).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Überschreiben"));
    await waitFor(() =>
      expect(sftpUploadFolder).toHaveBeenCalledWith(
        "session-1",
        "/local/proj",
        ".",
        ["proj/a.txt", "proj/b.txt"],
        null,
      ),
    );
  });

  it("cancelling the folder confirmation uploads nothing", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(pickUploadFolder).mockResolvedValue("/local/proj");
    vi.mocked(sftpUploadFolderPreview).mockResolvedValue(folderPreview(["proj/a.txt"]));

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Ordner hochladen"));
    await screen.findByText("Dateien im Ordner überschreiben?");
    fireEvent.click(screen.getByText("Abbrechen"));

    expect(screen.queryByText("Dateien im Ordner überschreiben?")).not.toBeInTheDocument();
    expect(sftpUploadFolder).not.toHaveBeenCalled();
  });

  it("the summary names skipped and failed entries", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(pickUploadFolder).mockResolvedValue("/local/proj");
    vi.mocked(sftpUploadFolderPreview).mockResolvedValue(folderPreview());
    vi.mocked(sftpUploadFolder).mockResolvedValue({
      ...folderSummary,
      skipped: [{ path: "link.txt", reason: "symlink" }],
      failed: [{ path: "locked.txt", error: "Permission denied" }],
    });

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Ordner hochladen"));

    expect(await screen.findByText("locked.txt")).toBeVisible();
    expect(screen.getByText("link.txt")).toBeVisible();
    expect(screen.getByText(/Permission denied/)).toBeVisible();
  });

  it("cancelling the folder dialog does nothing", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(pickUploadFolder).mockResolvedValue(null);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Ordner hochladen"));

    await waitFor(() => expect(pickUploadFolder).toHaveBeenCalledTimes(1));
    expect(sftpUploadFolderPreview).not.toHaveBeenCalled();
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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

  it("chown dialog sends only the changed side, recursive for folders (issue #654)", async () => {
    vi.mocked(sftpList).mockResolvedValue([{ ...dirEntry, owner: "alice", group: "staff" }]);
    vi.mocked(sftpChown).mockResolvedValue(4);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Besitzer ändern…"));

    const user = await screen.findByLabelText("Benutzer");
    expect(user).toHaveValue("alice");
    expect(screen.getByLabelText("Gruppe")).toHaveValue("staff");
    // Nothing changed yet: Apply is disabled.
    expect(screen.getByText("Übernehmen").closest("button")).toBeDisabled();
    fireEvent.change(user, { target: { value: "bob" } });
    const recursiveLabel = screen.getByText("Auf Inhalt anwenden");
    fireEvent.click(recursiveLabel.closest("label")!.querySelector("input")!);
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChown).toHaveBeenCalledWith("session-1", "logs", "bob", null, true, null),
    );
    await waitFor(() =>
      expect(lastToast()).toMatchObject({
        kind: "success",
        message: "Besitzer geändert — 4 Einträge in „logs“",
      }),
    );
  });

  it("chown reports an unknown user with its own message (issue #654)", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(sftpChown).mockRejectedValue({ message: "Unknown user: x", code: "CHOWN_UNKNOWN_USER" });

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Besitzer ändern…"));
    fireEvent.change(await screen.findByLabelText("Benutzer"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(lastToast()).toMatchObject({
        kind: "error",
        message: "Besitzer von „a.txt“ nicht geändert: unbekannter Benutzer",
      }),
    );
  });

  it("recursive chmod reports how many entries were changed", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(sftpChmod).mockResolvedValue(17);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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

  const enableElevation = async (alreadyRendered = false) => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: null,
    });
    if (!alreadyRendered) {
      renderPanel();
      await screen.findByText(/a\.txt/);
    }
    fireEvent.click(await screen.findByRole("button", { name: /Erhöhte Rechte/ }));
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

    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    fireEvent.click(await screen.findByRole("button", { name: /Erhöhte Rechte/ }));

    expect(
      await screen.findByText("deploy ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server"),
    ).toBeVisible();
    expect(screen.getByText(/passwortlosen Dateizugriff als root/)).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    // Issue #626: the sudoers line goes through the backend clipboard path.
    fireEvent.click(screen.getByRole("button", { name: "Kopieren" }));
    await waitFor(() =>
      expect(clipboardWriteText).toHaveBeenCalledWith(
        "deploy ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server",
      ),
    );
  });

  it("shows the unsafe sftp-server message and no sudoers line", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: false,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: { kind: "sftpServerUnsafe", sudoersLine: null, detail: "/usr/lib/openssh" },
    });
    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(await screen.findByRole("button", { name: /Erhöhte Rechte/ }));

    expect(await screen.findByText(/Nicht sicher: „\/usr\/lib\/openssh“ muss root gehören/)).toBeVisible();
    expect(screen.queryByText(/NOPASSWD/)).not.toBeInTheDocument();
    expect(screen.queryByText(/passwortlosen Dateizugriff/)).not.toBeInTheDocument();
  });

  it("warns in the file browser when elevated mode started with an unsafe sftp-server", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: null,
      warning: { kind: "sftpServerUnsafe", path: "/usr/lib/openssh/sftp-server" },
    });
    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(await screen.findByRole("button", { name: /Erhöhte Rechte/ }));

    expect(await screen.findByText(/Nicht sicher: „\/usr\/lib\/openssh\/sftp-server“/)).toBeVisible();
    expect(screen.getAllByRole("alert").length).toBe(2);
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
    fireEvent.click(await screen.findByRole("button", { name: "Weitere Optionen zur Rechteerhöhung" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Als anderer Nutzer/ }));
    fireEvent.change(screen.getByLabelText("Ziel-Nutzer"), { target: { value: "www-data" } });
    fireEvent.click(screen.getByRole("button", { name: "Erhöhen" }));

    await waitFor(() => expect(sftpElevationEnable).toHaveBeenCalledWith("session-1", "www-data", false));
    expect(await screen.findByRole("alert")).toHaveTextContent("www-data");
  });

  // Issue #544
  it("shows no text field in the toolbar and keeps root as the button's target", async () => {
    renderPanel();
    await screen.findByRole("button", { name: /Erhöhte Rechte/ });
    expect(screen.queryByLabelText("Ziel-Nutzer")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Weitere Optionen zur Rechteerhöhung" })).toBeVisible();
  });

  it("hides the whole elevation group for a root session", async () => {
    vi.mocked(sessionIsRoot).mockResolvedValueOnce(true);
    renderPanel();
    await screen.findByText(/a\.txt/);
    await waitFor(() => expect(sessionIsRoot).toHaveBeenCalledWith("session-1"));
    expect(screen.queryByRole("button", { name: /Erhöhte Rechte/ })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Weitere Optionen zur Rechteerhöhung" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Ziel-Nutzer")).not.toBeInTheDocument();
  });

  it("shows the group when the root state is unknown (null or probe error)", async () => {
    vi.mocked(sessionIsRoot).mockResolvedValueOnce(null);
    const first = renderPanel();
    expect(await screen.findByRole("button", { name: /Erhöhte Rechte/ })).toBeVisible();
    first.unmount();
    vi.mocked(sessionIsRoot).mockRejectedValueOnce("boom");
    renderPanel();
    expect(await screen.findByRole("button", { name: /Erhöhte Rechte/ })).toBeVisible();
  });

  it("closes the menu with Escape and returns focus to the arrow button; remembers the user", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: false,
      targetUser: "postgres",
      sftpServerPath: null,
      failure: { kind: "passwordRequired", sudoersLine: null, detail: null },
    });
    renderPanel();
    const arrow = await screen.findByRole("button", { name: "Weitere Optionen zur Rechteerhöhung" });
    await waitFor(() => expect(arrow).toBeEnabled());
    fireEvent.click(arrow);
    expect(screen.getByRole("menu")).toBeVisible();
    fireEvent.keyDown(screen.getByRole("menuitem"), { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    expect(arrow).toHaveFocus();

    fireEvent.click(arrow);
    fireEvent.click(screen.getByRole("menuitem"));
    fireEvent.change(screen.getByLabelText("Ziel-Nutzer"), { target: { value: "postgres" } });
    fireEvent.click(screen.getByRole("button", { name: "Erhöhen" }));
    await waitFor(() => expect(sftpElevationEnable).toHaveBeenCalledWith("session-1", "postgres", false));
    // The arrow is disabled until enabling has finished (`finally` after the
    // mock resolved); wait for that end state, not just for the call.
    await waitFor(() => expect(arrow).toBeEnabled());

    fireEvent.click(arrow);
    fireEvent.click(await screen.findByRole("menuitem"));
    expect(screen.getByLabelText("Ziel-Nutzer")).toHaveValue("postgres");
  });

  it("uses root when no target user is given", async () => {
    await enableElevation();
    expect(sftpElevationEnable).toHaveBeenCalledWith("session-1", "root", false);
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

    act(() => dragDrop.handler!({ payload: { type: "drop", position: IN, paths: ["/local/x.conf"] } }));

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

    act(() => handlerBeforeSwitch({ payload: { type: "drop", position: IN, paths: ["/local/x.conf"] } }));

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

    act(() => handlerWhileElevated({ payload: { type: "drop", position: IN, paths: ["/local/x.conf"] } }));

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
    expect(await screen.findByDisplayValue("logs")).toBeInTheDocument();

    act(() => handlerBeforeNavigation({ payload: { type: "drop", position: IN, paths: ["/local/x.conf"] } }));

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
    fireEvent.click(await screen.findByRole("button", { name: /Erhöhte Rechte/ }));

    await waitFor(() => expect(onElevationChange).toHaveBeenLastCalledWith("root"));
    fireEvent.click(screen.getByRole("button", { name: "Erhöhte Rechte beenden" }));
    await waitFor(() => expect(onElevationChange).toHaveBeenLastCalledWith(null));
  });

  // Issue #569
  describe("elevation menu closing", () => {
    const ARROW = "Weitere Optionen zur Rechteerhöhung";
    const openMenu = async () => {
      await screen.findByText(/a\.txt/);
      fireEvent.click(await screen.findByRole("button", { name: ARROW }));
      expect(screen.getByRole("menu")).toBeInTheDocument();
    };
    const openForm = async () => {
      await openMenu();
      fireEvent.click(screen.getByRole("menuitem", { name: /Als anderer Nutzer/ }));
      expect(screen.getByLabelText("Ziel-Nutzer")).toBeInTheDocument();
    };

    it("closes the menu on an outside click, but not on the arrow toggle or inside", async () => {
      renderPanel();
      await openMenu();
      fireEvent.click(screen.getByRole("menu"));
      expect(screen.getByRole("menu")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: ARROW }));
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: ARROW }));
      expect(screen.getByRole("menu")).toBeInTheDocument();
      fireEvent.click(screen.getByText(/a\.txt/));
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    });

    it("closes the target user form on an outside click without elevating", async () => {
      renderPanel();
      await openForm();
      fireEvent.click(screen.getByLabelText("Ziel-Nutzer"));
      expect(screen.getByLabelText("Ziel-Nutzer")).toBeInTheDocument();
      fireEvent.click(screen.getByText(/a\.txt/));
      expect(screen.queryByLabelText("Ziel-Nutzer")).not.toBeInTheDocument();
      expect(sftpElevationEnable).not.toHaveBeenCalled();
    });

    it("does not show the menu again after elevating and returning to normal rights", async () => {
      vi.mocked(sftpElevationEnable).mockResolvedValue({
        active: true,
        targetUser: "root",
        sftpServerPath: "/usr/lib/openssh/sftp-server",
        failure: null,
      });
      renderPanel();
      await openMenu();
      fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));
      fireEvent.click(await screen.findByRole("button", { name: "Erhöhte Rechte beenden" }));
      await screen.findByRole("button", { name: ARROW });
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
      expect(screen.queryByLabelText("Ziel-Nutzer")).not.toBeInTheDocument();
    });

    it("closes menu and form when the panel becomes hidden", async () => {
      const ui = (visible: boolean) => (
        <I18nextProvider i18n={testI18n}>
          <FileBrowserPanel sessionId="session-1" isVisible={visible} />
        </I18nextProvider>
      );
      const { rerender } = render(ui(true));
      await openMenu();
      rerender(ui(false));
      rerender(ui(true));
      expect(screen.queryByRole("menu")).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: ARROW }));
      fireEvent.click(screen.getByRole("menuitem", { name: /Als anderer Nutzer/ }));
      rerender(ui(false));
      rerender(ui(true));
      expect(screen.queryByLabelText("Ziel-Nutzer")).not.toBeInTheDocument();
    });
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

  const openEditAndChange = async () => {
    // Fake timers (see the ADR 0122, R7 note): the "changed" state hangs on
    // the real 2000 ms poll interval.
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Lokal öffnen…"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
  };
  const flushChange = async () => {
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:09Z");
    await act(async () => {
      await vi.runOnlyPendingTimersAsync();
    });
  };
  const arrangeEditMocks = () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/a.txt",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpReadText).mockResolvedValue("alt");
    vi.mocked(sftpStat).mockResolvedValue({ ...fileEntry, modified: "2026-01-01T00:00:00Z" });
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
  };
  const confirmDialog = async (title: RegExp) => {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const dialog = screen.getByText(title).closest(".fixed") as HTMLElement;
    fireEvent.click(within(dialog).getByRole("button", { name: "Hochladen" }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
  };

  it("elevated -> off (issue #656): explains the change, offers only 'Upload as login user', never uploads as the old identity without the click", async () => {
    arrangeEditMocks();
    await enableElevation();
    vi.useFakeTimers();
    try {
      await openEditAndChange();
      expect(sftpOpenForEditing).toHaveBeenCalledWith("session-1", "a.txt", "root");

      fireEvent.click(screen.getByRole("button", { name: "Erhöhte Rechte beenden" }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      await flushChange();

      const changed = screen.getByText(/wurde lokal geändert/);
      const bar = changed.parentElement!;
      expect(within(bar).getByText(/Als root geöffnet, der erhöhte Modus ist jetzt aus/)).toBeVisible();
      // No plain upload in this state.
      expect(within(bar).queryByRole("button", { name: "Hochladen" })).not.toBeInTheDocument();
      expect(sftpUpload).not.toHaveBeenCalled();

      fireEvent.click(within(bar).getByRole("button", { name: "Als Login-Nutzer hochladen" }));
      await confirmDialog(/Lokale Änderungen hochladen\?/);

      expect(sftpUpload).toHaveBeenCalledTimes(1);
      expect(sftpUpload).toHaveBeenCalledWith("session-1", "/tmp/edit/a.txt", "a.txt", null);
      // The session now belongs to the new identity: no mismatch banner left.
      expect(screen.queryByText(/der erhöhte Modus ist jetzt aus/)).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("off -> elevated (issue #656): offers 'Upload as root' and sends root explicitly", async () => {
    arrangeEditMocks();
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "root",
      sftpServerPath: "/usr/lib/openssh/sftp-server",
      failure: null,
    });
    renderPanel();
    await screen.findByText(/a\.txt/);
    vi.useFakeTimers();
    try {
      await openEditAndChange();
      expect(sftpOpenForEditing).toHaveBeenCalledWith("session-1", "a.txt", null);
      fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(screen.getByRole("alert")).toBeVisible();
      await flushChange();
      const bar = screen.getByText(/wurde lokal geändert/).parentElement!;
      expect(within(bar).getByText(/Ohne erhöhten Modus geöffnet, er läuft jetzt als root/)).toBeVisible();
      expect(within(bar).queryByRole("button", { name: "Hochladen" })).not.toBeInTheDocument();
      expect(sftpUpload).not.toHaveBeenCalled();

      fireEvent.click(within(bar).getByRole("button", { name: "Als root hochladen" }));
      await confirmDialog(/Lokale Änderungen als root hochladen\?/);

      expect(sftpUpload).toHaveBeenCalledWith("session-1", "/tmp/edit/a.txt", "a.txt", "root");
      expect(sftpUpload).not.toHaveBeenCalledWith("session-1", "/tmp/edit/a.txt", "a.txt", null);
    } finally {
      vi.useRealTimers();
    }
  });

  it("without a mismatch the plain 'Hochladen' stays (issue #656)", async () => {
    arrangeEditMocks();
    renderPanel();
    await screen.findByText(/a\.txt/);
    vi.useFakeTimers();
    try {
      await openEditAndChange();
      await flushChange();
      const bar = screen.getByText(/wurde lokal geändert/).parentElement!;
      expect(within(bar).getByRole("button", { name: "Hochladen" })).toBeVisible();
      expect(within(bar).queryByText(/Als .* hochladen/)).not.toBeInTheDocument();
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
      dragDrop.handler!({ payload: { type: "drop", position: IN, paths: ["/home/user/.ssh/id_ed25519"] } }),
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
    act(() => dragDrop.handler!({ payload: { type: "drop", position: IN, paths: ["/local/x.txt"] } }));

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
    act(() => dragDrop.handler!({ payload: { type: "drop", position: IN, paths: ["/local/project"] } }));

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
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
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
    act(() => dragDrop.handler!({ payload: { type: "over", position: IN } }));
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

  describe("transfer speed and remaining time (issue #623)", () => {
    async function setup() {
      let started: ((event: SftpTransferStartedEvent) => void) | null = null;
      const progress: ((event: any) => void)[] = [];
      vi.mocked(onSftpTransferStarted).mockImplementation((handler) => {
        started = handler;
        return Promise.resolve(() => {});
      });
      vi.mocked(onSftpTransferProgress).mockImplementation((hd: any) => {
        progress.push(hd);
        return Promise.resolve(() => {});
      });
      vi.mocked(sftpList).mockResolvedValue([fileEntry]);
      renderEn();
      await screen.findByText(/a\.txt/);
      await waitFor(() => expect(started).not.toBeNull());
      await waitFor(() => expect(progress.length).toBeGreaterThan(0));
      return { started: started!, emit: (e: any) => act(() => progress.forEach((fn) => fn(e))) };
    }
    const MB = 1024 * 1024;

    // Only the clock: restoreAllMocks would also reset the module mocks.
    let dateSpy: { mockRestore: () => void } | undefined;
    afterEach(() => dateSpy?.mockRestore());

    it("a file row shows speed and remaining time from the progress events", async () => {
      let now = 1_000_000;
      dateSpy = vi.spyOn(Date, "now").mockImplementation(() => now);
      const h = await setup();
      act(() =>
        h.started({ sessionId: "session-1", transferId: "t1", kind: "download", fileName: "f.bin", totalBytes: 10 * MB }),
      );
      h.emit({ sessionId: "session-1", transferId: "t1", bytesDone: 0, totalBytes: 10 * MB });
      now += 2000;
      h.emit({ sessionId: "session-1", transferId: "t1", bytesDone: 2 * MB, totalBytes: 10 * MB });
      expect(screen.getByText("1.0 MB/s")).toBeVisible();
      expect(screen.getByText("00:08 left")).toBeVisible();
    });

    it("a folder row shows the aggregate speed and remaining time", async () => {
      let now = 1_000_000;
      dateSpy = vi.spyOn(Date, "now").mockImplementation(() => now);
      const h = await setup();
      act(() =>
        h.started({ sessionId: "session-1", transferId: "t1", kind: "download", fileName: "dir", totalBytes: null }),
      );
      const folder = (bytesDone: number) => ({ filesDone: 1, filesTotal: 3, bytesDone, bytesTotal: 20 * MB });
      h.emit({ sessionId: "session-1", transferId: "t1", bytesDone: 0, totalBytes: 1, folder: folder(0) });
      now += 2000;
      // Per-file values are tiny; only the folder aggregate may drive the row.
      h.emit({ sessionId: "session-1", transferId: "t1", bytesDone: 1, totalBytes: 1, folder: folder(4 * MB) });
      expect(screen.getByText("2.0 MB/s")).toBeVisible();
      expect(screen.getByText("00:08 left")).toBeVisible();
    });

    it("an unknown total shows a dash for the remaining time", async () => {
      let now = 1_000_000;
      dateSpy = vi.spyOn(Date, "now").mockImplementation(() => now);
      const h = await setup();
      act(() =>
        h.started({ sessionId: "session-1", transferId: "t1", kind: "upload", fileName: "u.bin", totalBytes: null }),
      );
      h.emit({ sessionId: "session-1", transferId: "t1", bytesDone: 0, totalBytes: null });
      now += 2000;
      h.emit({ sessionId: "session-1", transferId: "t1", bytesDone: 2 * MB, totalBytes: null });
      expect(screen.getByText("1.0 MB/s")).toBeVisible();
      expect(screen.queryByText(/left/)).not.toBeInTheDocument();
    });
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
      // "Owner" is also a column header of the list behind the dialog.
      expect(screen.getAllByText(label).at(-1)).toBeVisible();
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
    for (const label of ["Owner", "Group", "Other"]) {
      expect(screen.getByRole("cell", { name: label })).toBeVisible();
    }
    expect(screen.getByLabelText("Owner: Write")).toBeInTheDocument();
    expect(screen.getByText("Numeric")).toBeVisible();
    expect(screen.getByText("Recursive")).toBeVisible();
    expect(screen.getByText(/changes the permissions of ALL files and subfolders/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Apply" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeVisible();
    expectNoGerman();
  });

  it("chmod dialog row labels are translated in a non-English locale", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);

    renderEn(testI18n.cloneInstance({ lng: "fr" }));
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: /^Actions: / }));
    fireEvent.click(screen.getByRole("button", { name: "Modifier les permissions…" }));

    for (const label of ["Propriétaire", "Groupe", "Autres"]) {
      expect(await screen.findByRole("cell", { name: label })).toBeVisible();
    }
    expect(screen.queryByRole("cell", { name: "Owner" })).not.toBeInTheDocument();
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
    vi.mocked(sftpReadText).mockRejectedValue({ message: "x", code: "SFTP_PREVIEW_NOT_TEXT" });
    vi.mocked(pickUploadFiles).mockResolvedValue(["/local/a.txt"]);

    renderEn();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Upload"));

    expect(await screen.findByRole("heading", { name: "Overwrite file?" })).toBeVisible();
    expect(screen.getByText(/already exists on the server\./)).toBeVisible();
    expect(
      screen.getByText(/^No text diff possible: the file is binary \(not valid UTF-8 text\)\.$/),
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
    vi.mocked(sftpReadText).mockRejectedValue({ message: "x", code: "SFTP_PREVIEW_NOT_TEXT" });

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
    expect(screen.getByText(/^No text diff possible: the file is binary \(not valid UTF-8 text\)\.$/)).toBeVisible();
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

describe("FileBrowserPanel drop routing by pointer position", () => {
  const HINT = new RegExp(`^${testI18n.t("fileBrowser.dropHint", { path: "\u0000" }).split("\u0000")[0]}`);
  beforeEach(() => {
    vi.mocked(sftpList).mockResolvedValue([]);
    dragDrop.handler = null;
    nativeDrop.paths = [];
  });
  afterEach(() => {
    Object.defineProperty(window, "devicePixelRatio", { value: 1, configurable: true });
  });

  it("claims a drop inside the panel, ignores one outside", async () => {
    renderPanel();
    await waitFor(() => expect(document.querySelector("[data-elevated]")).not.toBeNull());
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    vi.mocked(claimDroppedPaths).mockClear();
    act(() => dragDrop.handler!({ payload: { type: "drop", position: OUT, paths: ["/l/x"] } }));
    expect(claimDroppedPaths).not.toHaveBeenCalled();
    act(() => dragDrop.handler!({ payload: { type: "drop", position: IN, paths: ["/l/x"] } }));
    expect(claimDroppedPaths).toHaveBeenCalledTimes(1);
  });

  it("highlights only while over the panel and clears on leave / outside", async () => {
    renderPanel();
    await waitFor(() => expect(document.querySelector("[data-elevated]")).not.toBeNull());
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    act(() => dragDrop.handler!({ payload: { type: "over", position: IN } }));
    expect(screen.getByText(HINT)).toBeVisible();
    act(() => dragDrop.handler!({ payload: { type: "over", position: OUT } }));
    expect(screen.queryByText(HINT)).toBeNull();
    act(() => dragDrop.handler!({ payload: { type: "over", position: IN } }));
    act(() => dragDrop.handler!({ payload: { type: "leave" } }));
    expect(screen.queryByText(HINT)).toBeNull();
  });

  it("converts positions with devicePixelRatio", async () => {
    Object.defineProperty(window, "devicePixelRatio", { value: 2, configurable: true });
    renderPanel();
    await waitFor(() => expect(document.querySelector("[data-elevated]")).not.toBeNull());
    await waitFor(() => expect(dragDrop.handler).not.toBeNull());
    vi.mocked(claimDroppedPaths).mockClear();
    // 150 physical px = 75 CSS px: inside; with ratio 1 it would be outside.
    act(() => dragDrop.handler!({ payload: { type: "drop", position: { x: 150, y: 150 }, paths: [] } }));
    expect(claimDroppedPaths).toHaveBeenCalledTimes(1);
    act(() => dragDrop.handler!({ payload: { type: "drop", position: { x: 250, y: 250 }, paths: [] } }));
    expect(claimDroppedPaths).toHaveBeenCalledTimes(1);
  });
});

describe("FileBrowserPanel context menu on empty space (issue #570)", () => {
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
  });

  const emptySpace = (container: HTMLElement) => {
    const scroller = container.querySelector("table")!.parentElement!;
    return scroller;
  };

  it("opens the folder menu on empty space; a row still opens the entry menu", async () => {
    const { container } = renderPanel();
    const row = (await screen.findByText(/a\.txt/)).closest("tr")!;

    fireEvent.contextMenu(row);
    expect(screen.getByText("Umbenennen")).toBeVisible();
    expect(screen.queryByText("Neue Datei")).toBeNull();

    fireEvent.contextMenu(emptySpace(container), { clientX: 10, clientY: 20 });
    expect(screen.getByText("Neuer Ordner")).toBeVisible();
    expect(screen.getByText("Neue Datei")).toBeVisible();
    expect(screen.queryByText("Umbenennen")).toBeNull();
    // Without a pending cut there is no Paste entry in the menu.
    expect(screen.queryByRole("button", { name: "Einfügen" })).toBeNull();
  });

  it("offers Refresh on empty space and reloads the listing (issue #629)", async () => {
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    const before = vi.mocked(sftpList).mock.calls.length;
    fireEvent.contextMenu(emptySpace(container), { clientX: 10, clientY: 20 });
    fireEvent.click(screen.getByRole("button", { name: "Aktualisieren" }));
    await waitFor(() => expect(vi.mocked(sftpList).mock.calls.length).toBeGreaterThan(before));
    expect(screen.queryByText("Neue Datei")).toBeNull();
  });

  it("opens the folder menu in an empty directory and closes on Escape and outside click", async () => {
    vi.mocked(sftpList).mockResolvedValue([]);
    const { container } = renderPanel();
    const msg = await screen.findByText("(leeres Verzeichnis)");
    fireEvent.contextMenu(msg.parentElement!);
    expect(screen.getByText("Neue Datei")).toBeVisible();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByText("Neue Datei")).toBeNull();

    fireEvent.contextMenu(container.querySelector("p")!.parentElement!);
    expect(screen.getByText("Neue Datei")).toBeVisible();
    fireEvent.click(document.body);
    expect(screen.queryByText("Neue Datei")).toBeNull();
  });

  it("offers Paste in the menu while an entry is cut and moves it like the toolbar button", async () => {
    vi.mocked(sftpList).mockImplementation((_s, requestedPath) =>
      Promise.resolve(requestedPath === "sub" ? [] : [fileEntry, { ...fileEntry, name: "sub", path: "sub", isDir: true, permissionsOctal: 0o755 }]),
    );
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpRename).mockResolvedValue(undefined);
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getAllByRole("button", { name: /^(Aktionen|Actions): a\.txt/ })[0]);
    fireEvent.click(screen.getByText("Ausschneiden"));
    fireEvent.click(await screen.findByText(/sub/));
    await screen.findByText("(leeres Verzeichnis)");

    fireEvent.contextMenu(container.querySelector("p")!.parentElement!);
    const menu = screen.getByRole("group", { name: "Ordneraktionen" });
    fireEvent.click(within(menu).getByText("Einfügen"));

    await waitFor(() => expect(sftpRename).toHaveBeenCalledWith("session-1", "a.txt", "sub/a.txt", null));
  });

  it("New file creates an empty file in the current directory and refreshes", async () => {
    vi.mocked(sftpCreateFile).mockResolvedValue(undefined);
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.contextMenu(emptySpace(container));
    fireEvent.click(screen.getByText("Neue Datei"));
    fireEvent.change(within(await screen.findByRole("dialog")).getByRole("textbox"), { target: { value: "b.txt" } });
    vi.mocked(sftpList).mockClear();
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() => expect(sftpCreateFile).toHaveBeenCalledWith("session-1", "b.txt", null));
    await waitFor(() => expect(sftpList).toHaveBeenCalled());
    expect(showToast).toHaveBeenCalledWith(expect.objectContaining({ kind: "success" }));
  });

  it("New folder from the menu uses the same prompt as the toolbar button", async () => {
    vi.mocked(sftpMkdir).mockResolvedValue(undefined);
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.contextMenu(emptySpace(container));
    fireEvent.click(screen.getByText("Neuer Ordner"));
    fireEvent.change(within(await screen.findByRole("dialog")).getByRole("textbox"), { target: { value: "d" } });
    fireEvent.click(screen.getByText("Übernehmen"));
    await waitFor(() => expect(sftpMkdir).toHaveBeenCalledWith("session-1", "d", null));
  });

  it("rejects invalid names in the prompt without a backend call", async () => {
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    for (const bad of ["", "a/b", ".", ".."]) {
      fireEvent.contextMenu(emptySpace(container));
      fireEvent.click(screen.getByText("Neue Datei"));
      fireEvent.change(within(await screen.findByRole("dialog")).getByRole("textbox"), { target: { value: bad } });
      fireEvent.click(screen.getByText("Übernehmen"));
      expect(await screen.findByRole("alert")).toBeVisible();
      fireEvent.click(screen.getByText("Abbrechen"));
    }
    expect(sftpCreateFile).not.toHaveBeenCalled();
    expect(sftpMkdir).not.toHaveBeenCalled();
  });

  it("shows the translated 'already exists' and permission-denied reasons", async () => {
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    for (const [code, text] of [
      ["SSH_SFTP_ALREADY_EXISTS", /existiert bereits/],
      ["SSH_SFTP_PERMISSION_DENIED", /Zugriff verweigert/],
    ] as const) {
      vi.mocked(showToast).mockClear();
      vi.mocked(sftpCreateFile).mockRejectedValueOnce({ message: "roh", code });
      fireEvent.contextMenu(emptySpace(container));
      fireEvent.click(screen.getByText("Neue Datei"));
      fireEvent.change(within(await screen.findByRole("dialog")).getByRole("textbox"), { target: { value: "a.txt" } });
      fireEvent.click(screen.getByText("Übernehmen"));
      await waitFor(() =>
        expect(showToast).toHaveBeenCalledWith(
          expect.objectContaining({ kind: "error", message: expect.stringMatching(text) }),
        ),
      );
    }
    vi.mocked(sftpMkdir).mockRejectedValueOnce({ message: "roh", code: "SSH_SFTP_ALREADY_EXISTS" });
    fireEvent.contextMenu(emptySpace(container));
    fireEvent.click(screen.getByText("Neuer Ordner"));
    fireEvent.change(within(await screen.findByRole("dialog")).getByRole("textbox"), { target: { value: "a.txt" } });
    fireEvent.click(screen.getByText("Übernehmen"));
    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(
        expect.objectContaining({ message: expect.stringMatching(/existiert bereits/) }),
      ),
    );
  });

  describe("paste of local clipboard files (issue #571)", () => {
    beforeEach(() => {
      vi.mocked(clipboardHasFiles).mockResolvedValue(false);
      vi.mocked(sftpExists).mockResolvedValue(false);
      vi.mocked(sftpUpload).mockResolvedValue(undefined);
    });

    it("offers Paste only when the clipboard holds files", async () => {
      const { container } = renderPanel();
      await screen.findByText(/a\.txt/);
      fireEvent.contextMenu(emptySpace(container));
      await act(async () => {});
      expect(screen.queryByRole("button", { name: "Einfügen" })).toBeNull();

      vi.mocked(clipboardHasFiles).mockResolvedValue(true);
      fireEvent.keyDown(document, { key: "Escape" });
      fireEvent.contextMenu(emptySpace(container));
      expect(await screen.findByRole("button", { name: "Einfügen" })).toBeVisible();
    });

    it("hands the confirmed paths to the normal upload flow", async () => {
      vi.mocked(clipboardHasFiles).mockResolvedValue(true);
      vi.mocked(claimClipboardFiles).mockResolvedValue(["/home/u/new.txt"]);
      const { container } = renderPanel();
      await screen.findByText(/a\.txt/);
      fireEvent.contextMenu(emptySpace(container));
      fireEvent.click(await screen.findByRole("button", { name: "Einfügen" }));

      await waitFor(() =>
        expect(sftpUpload).toHaveBeenCalledWith("session-1", "/home/u/new.txt", "new.txt", null),
      );
      // Only the target directory goes to the backend, never a local path.
      expect(claimClipboardFiles).toHaveBeenCalledWith("session-1", ".", expect.any(Object));
    });

    it("uploads nothing when the dialog was cancelled", async () => {
      vi.mocked(clipboardHasFiles).mockResolvedValue(true);
      vi.mocked(claimClipboardFiles).mockResolvedValue([]);
      const { container } = renderPanel();
      await screen.findByText(/a\.txt/);
      fireEvent.contextMenu(emptySpace(container));
      fireEvent.click(await screen.findByRole("button", { name: "Einfügen" }));
      await waitFor(() => expect(claimClipboardFiles).toHaveBeenCalled());
      await act(async () => {});
      expect(sftpExists).not.toHaveBeenCalled();
      expect(sftpUpload).not.toHaveBeenCalled();
      expect(showToast).not.toHaveBeenCalled();
    });

    it("shows an info toast when the clipboard emptied in between", async () => {
      vi.mocked(clipboardHasFiles).mockResolvedValue(true);
      vi.mocked(claimClipboardFiles).mockResolvedValue(null);
      const { container } = renderPanel();
      await screen.findByText(/a\.txt/);
      fireEvent.contextMenu(emptySpace(container));
      fireEvent.click(await screen.findByRole("button", { name: "Einfügen" }));
      await waitFor(() =>
        expect(showToast).toHaveBeenCalledWith(expect.objectContaining({ kind: "info" })),
      );
      expect(sftpUpload).not.toHaveBeenCalled();
    });

    it("a pending cut keeps Paste as a move and does not read the clipboard", async () => {
      vi.mocked(clipboardHasFiles).mockResolvedValue(true);
      vi.mocked(sftpRename).mockResolvedValue(undefined);
      const { container } = renderPanel();
      const row = (await screen.findByText(/a\.txt/)).closest("tr")!;
      fireEvent.contextMenu(row);
      fireEvent.click(screen.getByText("Ausschneiden"));
      vi.mocked(clipboardHasFiles).mockClear();

      fireEvent.contextMenu(emptySpace(container));
      fireEvent.click(
        within(await screen.findByRole("group", { name: "Ordneraktionen" })).getByRole("button", {
          name: "Einfügen",
        }),
      );
      expect(clipboardHasFiles).not.toHaveBeenCalled();
      expect(claimClipboardFiles).not.toHaveBeenCalled();
    });
  });

  // Issue #621: the transfer listeners must survive an elevation switch.
  describe("transfer rows across an elevation switch (issue #621)", () => {
    type Handler<T> = (event: T) => void;
    const enableElevation = async () => {
      vi.mocked(sftpElevationEnable).mockResolvedValue({
        active: true,
        targetUser: "root",
        sftpServerPath: "/usr/lib/openssh/sftp-server",
        failure: null,
      });
      renderPanel();
      await screen.findByText(/a\.txt/);
      fireEvent.click(await screen.findByRole("button", { name: /Erhöhte Rechte/ }));
      await screen.findByRole("alert");
    };
    beforeEach(() => {
      vi.clearAllMocks();
      vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
      vi.mocked(sftpList).mockResolvedValue([{ ...entry, name: "a.txt", path: "a.txt" }]);
    });
    const subscribe = () => {
      const started: Handler<any>[] = [];
      const finished: Handler<any>[] = [];
      let subscriptions = 0;
      vi.mocked(onSftpTransferStarted).mockImplementation((h: any) => {
        subscriptions++;
        started.push(h);
        return Promise.resolve(() => {
          started.splice(started.indexOf(h), 1);
        });
      });
      vi.mocked(onSftpTransferFinished).mockImplementation((h: any) => {
        finished.push(h);
        return Promise.resolve(() => {
          // Unsubscribing is async: the old handler is gone before a new one
          // is registered (the window in which events used to get lost).
          finished.splice(finished.indexOf(h), 1);
        });
      });
      return { started, finished, subscriptions: () => subscriptions };
    };
    const start = (h: ReturnType<typeof subscribe>, id = "t1") =>
      act(() =>
        h.started.forEach((fn) =>
          fn({ sessionId: "session-1", transferId: id, kind: "download", fileName: "big.bin", totalBytes: 10 }),
        ),
      );
    const finish = (h: ReturnType<typeof subscribe>, error: string | null, id = "t1") =>
      act(() => h.finished.forEach((fn) => fn({ sessionId: "session-1", transferId: id, error })));

    it("a transfer finishing right after elevation is switched off ends as done, with a note", async () => {
      const h = subscribe();
      await enableElevation();
      const before = h.subscriptions();
      start(h);
      expect(screen.getByText(/Herunterladen: big\.bin/)).toBeVisible();

      fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));
      await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
      expect(h.subscriptions()).toBe(before); // no re-subscription
      finish(h, null);

      expect(screen.queryByText(/Herunterladen: big\.bin/)).not.toBeInTheDocument();
      expect(screen.getByText(/Fertig: big\.bin/)).toBeVisible();
      expect(screen.getByText(/erhöhte Modus wurde während der Übertragung ausgeschaltet/)).toBeVisible();
    });

    it("a transfer without an elevation change disappears when finished", async () => {
      const h = subscribe();
      renderPanel();
      await screen.findByText(/a\.txt/);
      await waitFor(() => expect(h.finished.length).toBe(1));
      start(h);
      finish(h, null);
      expect(screen.queryByText(/big\.bin/)).not.toBeInTheDocument();
    });

    it("a revoked transfer shows the inactive-channel message without the channel-error prefix", async () => {
      const h = subscribe();
      await enableElevation();
      start(h);
      fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte/ }));
      await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
      finish(h, "Erhöhte Rechte beendet — der erhöhte Kanal ist nicht mehr aktiv.");

      expect(screen.getByText(/big\.bin: Erhöhte Rechte beendet/)).toBeVisible();
      expect(screen.queryByText(/Channel-Fehler/)).not.toBeInTheDocument();
    });

    // Issue #622
    const running = [
      { id: "op1", kind: "download" as const, path: "/srv/big.bin", bytesDone: 5, bytesTotal: 10, filesDone: null, filesTotal: null },
      { id: "op2", kind: "delete" as const, path: "/srv/old", bytesDone: null, bytesTotal: null, filesDone: null, filesTotal: null },
    ];

    it("ending elevated rights with running operations asks first; Cancel keeps the mode on", async () => {
      await enableElevation();
      vi.mocked(sftpElevationDisable).mockResolvedValueOnce({ disabled: false, runningOperations: running });
      fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte beenden/ }));

      expect(await screen.findByText("Erhöhte Rechte beenden?")).toBeVisible();
      expect(screen.getByText("/srv/big.bin")).toBeVisible();
      expect(screen.getByText("/srv/old")).toBeVisible();
      fireEvent.click(screen.getByRole("button", { name: "Abbrechen" }));

      await waitFor(() => expect(screen.queryByText("Erhöhte Rechte beenden?")).not.toBeInTheDocument());
      expect(screen.getByRole("alert")).toBeVisible();
      expect(sftpElevationDisable).not.toHaveBeenCalledWith("session-1", true);
    });

    it("confirming the dialog stops the operations and switches the mode off", async () => {
      await enableElevation();
      vi.mocked(sftpElevationDisable).mockResolvedValueOnce({ disabled: false, runningOperations: running });
      fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte beenden/ }));
      await screen.findByText("Erhöhte Rechte beenden?");
      const confirm = screen.getAllByRole("button", { name: /Erhöhte Rechte beenden/ }).at(-1)!;
      fireEvent.click(confirm);

      await waitFor(() => expect(sftpElevationDisable).toHaveBeenCalledWith("session-1", true));
      await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    });

    it("ending elevated rights without running operations needs no dialog", async () => {
      await enableElevation();
      fireEvent.click(screen.getByRole("button", { name: /Erhöhte Rechte beenden/ }));
      await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
      expect(screen.queryByText("Erhöhte Rechte beenden?")).not.toBeInTheDocument();
    });

    it("a cancelled transfer shows its state, a running one has a cancel button and progress", async () => {
      const h = subscribe();
      const progress: Handler<any>[] = [];
      vi.mocked(onSftpTransferProgress).mockImplementation((hd: any) => {
        progress.push(hd);
        return Promise.resolve(() => {});
      });
      renderPanel();
      await screen.findByText(/a\.txt/);
      await waitFor(() => expect(h.finished.length).toBe(1));
      start(h);
      act(() =>
        progress.forEach((fn) =>
          fn({ sessionId: "session-1", transferId: "t1", bytesDone: 4, totalBytes: 10 }),
        ),
      );
      expect(screen.getByRole("progressbar", { name: "big.bin" })).toHaveAttribute("value", "4");
      fireEvent.click(screen.getByRole("button", { name: "Übertragung abbrechen" }));
      expect(sftpCancelTransfer).toHaveBeenCalledWith("session-1", "t1");

      act(() =>
        h.finished.forEach((fn) =>
          fn({ sessionId: "session-1", transferId: "t1", error: null, stopped: "cancelled" }),
        ),
      );
      expect(screen.getByText("big.bin: abgebrochen")).toBeVisible();
    });

    it("a recursive delete row shows entries of total, cancels, and ends with the count (issue #658)", async () => {
      const h = subscribe();
      const progress: Handler<any>[] = [];
      vi.mocked(onSftpTransferProgress).mockImplementation((hd: any) => {
        progress.push(hd);
        return Promise.resolve(() => {});
      });
      renderPanel();
      await screen.findByText(/a\.txt/);
      await waitFor(() => expect(h.finished.length).toBe(1));
      act(() =>
        h.started.forEach((fn) =>
          fn({ sessionId: "session-1", transferId: "d1", kind: "delete", fileName: "/tmp/gross", totalBytes: null }),
        ),
      );
      const emit = (done: number, total: number | null) =>
        act(() =>
          progress.forEach((fn) =>
            fn({ sessionId: "session-1", transferId: "d1", bytesDone: 0, totalBytes: null, entries: { done, total } }),
          ),
        );
      emit(4120, 10951);
      expect(screen.getByText(/Löschen: \/tmp\/gross/)).toBeVisible();
      expect(screen.getByText("4120 von 10951 Einträgen")).toBeVisible();
      expect(screen.getByRole("progressbar", { name: "/tmp/gross" })).toHaveAttribute("value", "4120");
      emit(100, 10951); // stale event: counts never go backwards
      expect(screen.getByText("4120 von 10951 Einträgen")).toBeVisible();

      fireEvent.click(screen.getByRole("button", { name: "Übertragung abbrechen" }));
      expect(sftpCancelTransfer).toHaveBeenCalledWith("session-1", "d1");
      act(() =>
        h.finished.forEach((fn) =>
          fn({ sessionId: "session-1", transferId: "d1", error: null, stopped: "cancelled", entriesDone: 4200 }),
        ),
      );
      expect(screen.getByText(/\/tmp\/gross: abgebrochen/)).toBeVisible();
      expect(screen.getByText(/Geänderte Einträge: 4200/)).toBeVisible();
    });
  });
});

describe("FileBrowserPanel menus stay inside the window (issue #627)", () => {
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };
  const origW = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetWidth");
  const origH = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    window.innerWidth = 1000;
    window.innerHeight = 600;
    // jsdom has no layout: every measured element is 200 x 300.
    Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 200 });
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 300 });
  });
  afterEach(() => {
    if (origW) Object.defineProperty(HTMLElement.prototype, "offsetWidth", origW);
    if (origH) Object.defineProperty(HTMLElement.prototype, "offsetHeight", origH);
  });

  const menuOf = (label: string) => screen.getByText(label).closest("div,fieldset") as HTMLElement;

  it("opens the row context menu where the click is when there is room", async () => {
    renderPanel();
    const row = (await screen.findByText(/a\.txt/)).closest("tr")!;
    fireEvent.contextMenu(row, { clientX: 100, clientY: 100 });
    expect(menuOf("Umbenennen")).toHaveStyle({ left: "100px", top: "100px" });
  });

  it("opens the row context menu upward at the bottom edge", async () => {
    renderPanel();
    const row = (await screen.findByText(/a\.txt/)).closest("tr")!;
    fireEvent.contextMenu(row, { clientX: 100, clientY: 590 });
    expect(menuOf("Umbenennen")).toHaveStyle({ top: "290px" });
  });

  it("opens the row context menu to the left at the right edge", async () => {
    renderPanel();
    const row = (await screen.findByText(/a\.txt/)).closest("tr")!;
    fireEvent.contextMenu(row, { clientX: 995, clientY: 100 });
    expect(menuOf("Umbenennen")).toHaveStyle({ left: "792px" });
  });

  it("keeps the empty-space menu inside the window in the bottom right corner", async () => {
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    const scroller = container.querySelector("table")!.parentElement!;
    fireEvent.contextMenu(scroller, { clientX: 995, clientY: 595 });
    expect(menuOf("Neuer Ordner")).toHaveStyle({ left: "792px", top: "295px" });
  });

  it("shifts the target user form inside the window near the right edge", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: true,
      targetUser: "root",
      sftpServerPath: "",
      failure: null,
    });
    // The elevation group sits at the right window edge.
    const orig = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
      if (this.tagName === "FIELDSET" && this.getAttribute("aria-label")?.includes("Rechte")) {
        return { left: 900, top: 10, right: 990, bottom: 30, width: 90, height: 20, x: 900, y: 10, toJSON() {} } as DOMRect;
      }
      return orig.call(this);
    };
    try {
      renderPanel();
      await screen.findByText(/a\.txt/);
      fireEvent.click(await screen.findByRole("button", { name: "Weitere Optionen zur Rechteerhöhung" }));
      fireEvent.click(screen.getByRole("menuitem", { name: /Als anderer Nutzer/ }));
      const form = screen.getByLabelText("Ziel-Nutzer").closest("form") as HTMLElement;
      // 900 + 200 would stick out; end-aligned: 990 - 200 = 790.
      expect(form).toHaveStyle({ left: "790px" });
    } finally {
      HTMLElement.prototype.getBoundingClientRect = orig;
    }
  });
});

describe("FileBrowserPanel header sorting (Spec 0053, Teil 3)", () => {
  const small = { ...entry, name: "a.txt", path: "a.txt", size: 1 };
  const big = { ...entry, name: "b.txt", path: "b.txt", size: 999 };
  const rowNames = (container: HTMLElement) =>
    Array.from(container.querySelectorAll("tbody tr td:first-child")).map((td) => td.textContent?.trim());

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(loadFileManagerSort).mockResolvedValue(null);
    vi.mocked(sftpList).mockResolvedValue([small, big]);
  });

  it("sorts on click, reverses on second click, persists and sets aria-sort", async () => {
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    expect(screen.getByTestId("column-sort-name").closest("th")).toHaveAttribute("aria-sort", "ascending");

    fireEvent.click(screen.getByTestId("column-sort-size"));
    expect(saveFileManagerSort).toHaveBeenLastCalledWith({ column: "size", direction: "asc" });
    expect(screen.getByTestId("column-sort-size").closest("th")).toHaveAttribute("aria-sort", "ascending");
    expect(screen.getByTestId("column-sort-name").closest("th")).toHaveAttribute("aria-sort", "none");
    expect(rowNames(container)).toEqual(["📄 a.txt", "📄 b.txt"]);

    fireEvent.click(screen.getByTestId("column-sort-size"));
    expect(saveFileManagerSort).toHaveBeenLastCalledWith({ column: "size", direction: "desc" });
    expect(rowNames(container)).toEqual(["📄 b.txt", "📄 a.txt"]);
  });

  it("applies the persisted sort on mount", async () => {
    vi.mocked(loadFileManagerSort).mockResolvedValue({ column: "size", direction: "desc" });
    const { container } = renderPanel();
    await screen.findByText(/a\.txt/);
    await waitFor(() => expect(rowNames(container)).toEqual(["📄 b.txt", "📄 a.txt"]));
  });

  it("a resize handle click never changes the sort", async () => {
    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByTestId("column-resize-size"));
    expect(saveFileManagerSort).not.toHaveBeenCalled();
  });
});

// Issue #655: errors of the edit flow's upload show only in the edit footer.
describe("FileBrowserPanel edit flow errors (issue #655)", () => {
  const fileEntry: RemoteEntryDto = { ...entry, name: "a.txt", path: "a.txt" };
  type Handler<T> = ((event: T) => void) | null;
  let started: Handler<SftpTransferStartedEvent> = null;
  let finished: Handler<any> = null;

  async function openChangedEdit(readText: () => Promise<string>) {
    started = null;
    finished = null;
    vi.mocked(onSftpTransferStarted).mockImplementation((h: any) => {
      started = h;
      return Promise.resolve(() => {});
    });
    vi.mocked(onSftpTransferFinished).mockImplementation((h: any) => {
      finished = h;
      return Promise.resolve(() => {});
    });
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/a.txt",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime)
      .mockResolvedValueOnce("2026-01-01T00:00:01Z")
      .mockResolvedValue("2026-01-01T00:00:09Z");
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpStat).mockResolvedValue({ ...entry, modified: "2026-01-01T00:00:00Z" });
    vi.mocked(sftpReadText).mockImplementation(readText);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
    fireEvent.click(screen.getByText("Lokal öffnen…"));
    // The poll runs every 2 s and sees the changed mtime.
    await screen.findByText(/wurde lokal geändert/, undefined, { timeout: 5000 });
  }

  /** The upload fails after the backend announced a transfer (as it does). */
  function failingUpload(message: string) {
    vi.mocked(sftpUpload).mockImplementation(async () => {
      const transferId = `t-${Math.random()}`;
      act(() => {
        started?.({ sessionId: "session-1", transferId, kind: "upload", fileName: "a.txt", totalBytes: 3 });
        finished?.({ sessionId: "session-1", transferId, error: message, stopped: null });
      });
      throw { message };
    });
  }

  async function uploadFromFooter() {
    const footer = screen.getByText(/wurde lokal geändert/).parentElement!;
    fireEvent.click(within(footer).getByText("Hochladen"));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByText("Hochladen"));
  }

  it("a failed edit upload shows exactly one error, in the footer; a retry replaces it", async () => {
    await openChangedEdit(() => Promise.resolve("alt"));
    failingUpload("Zugriff verweigert");

    await uploadFromFooter();
    await waitFor(() => expect(screen.getAllByText(/Zugriff verweigert/)).toHaveLength(1));
    expect(showToast).not.toHaveBeenCalledWith(expect.objectContaining({ kind: "error" }));
    // No transfer row ("a.txt: <error>") for the edit upload.
    expect(screen.queryByText(/a\.txt: Zugriff verweigert/)).not.toBeInTheDocument();

    await uploadFromFooter();
    await waitFor(() => expect(sftpUpload).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getAllByText(/Zugriff verweigert/)).toHaveLength(1));
    expect(showToast).not.toHaveBeenCalledWith(expect.objectContaining({ kind: "error" }));
  }, 15000);

  it("a normal upload keeps its transfer row", async () => {
    await openChangedEdit(() => Promise.resolve("alt"));
    // A different file is uploaded while the edit session is idle.
    act(() => {
      started!({ sessionId: "session-1", transferId: "n1", kind: "upload", fileName: "other.txt", totalBytes: 3 });
      finished!({ sessionId: "session-1", transferId: "n1", error: "kaputt", stopped: null });
    });
    expect(screen.getByText(/other\.txt: kaputt/)).toBeVisible();
  }, 15000);

  it("a successful edit upload still shows the success toast", async () => {
    await openChangedEdit(() => Promise.resolve("alt"));
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    await uploadFromFooter();
    await waitFor(() =>
      expect(showToast).toHaveBeenCalledWith(expect.objectContaining({ kind: "success" })),
    );
  }, 15000);

  it.each([
    [{ message: "x", code: "SSH_SFTP_PERMISSION_DENIED" }, /Leseberechtigung/],
    [{ message: "x", code: "SFTP_PREVIEW_TOO_LARGE" }, /zu groß \(über 256 KB\)/],
    [{ message: "x", code: "SFTP_PREVIEW_NOT_TEXT" }, /Datei ist binär/],
    [{ message: "Verbindung weg" }, /nicht gelesen werden \(Verbindung weg\)/],
  ])("the diff hint names the read error %#", async (err, hint) => {
    await openChangedEdit(() => Promise.reject(err));
    const footer = screen.getByText(/wurde lokal geändert/).parentElement!;
    fireEvent.click(within(footer).getByText("Hochladen"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(hint)).toBeVisible();
  }, 15000);

  it("the upload-conflict dialog names a permission error too", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(true);
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpReadText).mockRejectedValue({ message: "x", code: "SSH_SFTP_PERMISSION_DENIED" });
    vi.mocked(pickUploadFiles).mockResolvedValue(["/local/a.txt"]);
    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Hochladen"));
    expect(await screen.findByText(/Leseberechtigung/)).toBeVisible();
  });
});
