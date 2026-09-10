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
import { open } from "@tauri-apps/plugin-dialog";
import {
  readLocalTextPreview,
  sftpChmod,
  sftpDeletePreview,
  sftpDownload,
  sftpDownloadDefault,
  sftpDownloadDir,
  sftpExists,
  sftpList,
  sftpReadText,
  sftpRename,
  sftpUpload,
} from "../api";
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
  sftpDeletePreview: vi.fn(),
  sftpDownload: vi.fn(),
  sftpDownloadDefault: vi.fn(),
  sftpDownloadDir: vi.fn(),
  sftpMkdir: vi.fn(),
  sftpReadText: vi.fn(),
  sftpRename: vi.fn(),
  sftpUpload: vi.fn(),
  sftpExists: vi.fn(),
  sftpChmod: vi.fn(),
  readLocalTextPreview: vi.fn(),
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

    await waitFor(() => expect(sftpList).toHaveBeenCalledWith("session-1", "logs"));
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
    vi.mocked(sftpDownloadDefault).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Herunterladen"));

    expect(sftpDownloadDefault).toHaveBeenCalledWith("session-1", "a.txt");
  });

  it("'Herunterladen nach…' uses the folder dialog for directories, file dialog for files", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpDownloadDir).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Herunterladen nach…"));

    expect(sftpDownloadDir).toHaveBeenCalledWith("session-1", "logs");
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
    expect(await screen.findByText("Pfad kopiert")).toBeVisible();
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
    expect(await screen.findByText("Inhalt kopiert")).toBeVisible();
  });

  it("shows a toast with the backend error when the file is not text", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpReadText).mockRejectedValue("Datei ist keine Textdatei (kein gültiges UTF-8)");

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Dateiinhalt kopieren"));

    expect(await screen.findByText("Datei ist keine Textdatei (kein gültiges UTF-8)")).toBeVisible();
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
    expect(sftpList).toHaveBeenLastCalledWith("session-1", ".");
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
    vi.mocked(sftpChmod).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    expect(await screen.findByText("Rechte bearbeiten")).toBeVisible();
    // fileEntry ist 644 (rw-r--r--) — "Owner Schreiben" (0o200) abwählen.
    fireEvent.click(screen.getByLabelText("Owner 128"));
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "a.txt", 0o444, false),
    );
  });

  it("chmod dialog's numeric input overrides the checkbox matrix", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    const numeric = await screen.findByDisplayValue("644");
    fireEvent.change(numeric, { target: { value: "700" } });
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "a.txt", 0o700, false),
    );
  });

  it("chmod dialog shows a recursive checkbox only for directories", async () => {
    vi.mocked(sftpList).mockResolvedValue([dirEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpChmod).mockResolvedValue(undefined);

    renderPanel();
    await screen.findByText(/logs/);
    fireEvent.click(screen.getByRole("button", { name: "⋮" }));
    fireEvent.click(screen.getByText("Rechte bearbeiten…"));

    const recursiveLabel = await screen.findByText(/Rekursiv/);
    fireEvent.click(recursiveLabel.closest("label")!.querySelector("input")!);
    fireEvent.click(screen.getByText("Übernehmen"));

    await waitFor(() =>
      expect(sftpChmod).toHaveBeenCalledWith("session-1", "logs", 0o755, true),
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
    await waitFor(() => expect(sftpDeletePreview).toHaveBeenCalledWith("session-1", "logs"));
    expect(await screen.findByText("3")).toBeVisible();
    expect(screen.getByText("2")).toBeVisible();
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

    await waitFor(() => expect(sftpExists).toHaveBeenCalledWith("session-1", "b.txt"));
    expect(await screen.findByText("Ziel existiert bereits")).toBeVisible();
    expect(sftpRename).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("Überschreiben"));
    await waitFor(() => expect(sftpRename).toHaveBeenCalledWith("session-1", "a.txt", "b.txt"));
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

    await waitFor(() => expect(sftpRename).toHaveBeenCalledWith("session-1", "a.txt", "b.txt"));
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

    await waitFor(() => expect(sftpExists).toHaveBeenCalledWith("session-1", "logs/a.txt"));
    expect(sftpRename).toHaveBeenCalledWith("session-1", "a.txt", "logs/a.txt");
  });

  it("upload onto an existing text file shows a diff, confirming uploads it", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(true);
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpReadText).mockResolvedValue("alt");
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    vi.mocked(open).mockResolvedValue("/local/a.txt");

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
      expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/a.txt", "a.txt"),
    );
  });

  it("upload to a new path skips the conflict dialog entirely", async () => {
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(sftpExists).mockResolvedValue(false);
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    vi.mocked(open).mockResolvedValue("/local/new.txt");

    renderPanel();
    await screen.findByText(/a\.txt/);
    fireEvent.click(screen.getByText("Hochladen"));

    await waitFor(() =>
      expect(sftpUpload).toHaveBeenCalledWith("session-1", "/local/new.txt", "new.txt"),
    );
    expect(screen.queryByText("Datei überschreiben?")).not.toBeInTheDocument();
    expect(readLocalTextPreview).not.toHaveBeenCalled();
  });
});
