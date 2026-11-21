import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpList, sftpStartDirectory } from "../api/api";
import { loadHiddenFileColumns, saveHiddenFileColumns } from "../settings/layoutSettings";
import { testI18n } from "../testI18n";
import type { RemoteEntryDto } from "../api/types";
import { FileBrowserPanel } from "./FileBrowserPanel";

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    sftpList: vi.fn(),
    sftpStartDirectory: vi.fn(),
    sessionIsRoot: vi.fn(() => Promise.resolve(false)),
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
    sftpReadText: vi.fn(),
    sftpRename: vi.fn(),
    sftpUpload: vi.fn(),
    sftpExists: vi.fn(),
    sftpChmod: vi.fn(),
    sftpChown: vi.fn(),
    readLocalTextPreview: vi.fn(),
    sftpStat: vi.fn(),
    sftpOpenForEditing: vi.fn(),
    localFileMtime: vi.fn(),
    closeEditSession: vi.fn(),
  }),
);

vi.mock("../api/events", () => ({
  onConnectionStatusChanged: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferStarted: vi.fn(() => Promise.resolve(() => {})),
  onElevationAutoDisabled: vi.fn(() => Promise.resolve(() => {})),
  onElevationRevokedByProfile: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferFinished: vi.fn(() => Promise.resolve(() => {})),
  onSftpTransferProgress: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn(),
  revealItemInDir: vi.fn(() => Promise.resolve()),
}));

vi.mock("../buses/toastBus", () => ({
  showToast: vi.fn(),
}));

vi.mock("../settings/fileTypeSettings", () => ({
  loadFileTypeApps: vi.fn(() => Promise.resolve({})),
  appForFileName: () => null,
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn(() => Promise.resolve(() => {})),
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("../settings/layoutSettings", () => ({
  loadFileManagerColumnWidths: vi.fn(() => Promise.resolve({})),
  saveFileManagerColumnWidths: vi.fn(() => Promise.resolve()),
  loadHiddenFileColumns: vi.fn(() => Promise.resolve([])),
  saveHiddenFileColumns: vi.fn(() => Promise.resolve()),
  loadFileManagerSort: vi.fn(() => Promise.resolve(null)),
  saveFileManagerSort: vi.fn(() => Promise.resolve()),
}));

function dirEntry(path: string): RemoteEntryDto {
  return {
    name: path.split("/").pop() ?? path,
    path,
    isDir: true,
    size: 0,
    permissions: "rwxr-xr-x",
    modified: null,
    permissionsOctal: 0o755,
    uid: 1000,
    gid: 1000,
    owner: null,
    group: null, isSymlink: false, linkTarget: null, linkKind: null,
  };
}

function renderPanel() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <FileBrowserPanel sessionId="session-1" isVisible={true} />
    </I18nextProvider>,
  );
}

// Issue #1373 (Spec 0020 §5/§7, Spec 0053): breadcrumb path bar that turns into
// the editable input, columns toolbar button and the inline link target.
const link: RemoteEntryDto = {
  ...dirEntry("/var/www/shop/current"),
  name: "current",
  isDir: false,
  isSymlink: true,
  linkTarget: "/var/www/shop/releases/42",
  linkKind: "file",
};

const bar = () => screen.getByTestId("path-bar");

describe("FileBrowserPanel path bar (issue #1373)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(sftpList).mockResolvedValue([link]);
    vi.mocked(sftpStartDirectory).mockResolvedValue({ path: "/var/www/shop", missingDirectory: null });
  });

  it("clicking a breadcrumb segment of /var/www/shop loads /var/www", async () => {
    renderPanel();
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var/www/shop"));
    const segments = within(bar()).getAllByRole("button").filter((b) => b.getAttribute("aria-hidden") !== "true");
    expect(segments.map((b) => b.textContent)).toEqual(["/", "var", "www", "shop"]);
    expect(segments[3]).toHaveAttribute("aria-current", "location");
    fireEvent.click(segments[2]);
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", "/var/www", null));
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var/www"));
  });

  it("edit mode: typing /etc and Enter loads /etc; Escape restores the breadcrumb", async () => {
    renderPanel();
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var/www/shop"));
    fireEvent.click(screen.getByTestId("path-bar-edit"));
    const input = await screen.findByRole("textbox", { name: "Pfad" });
    expect(input).toHaveFocus();
    fireEvent.change(input, { target: { value: "/etc" } });
    fireEvent.keyDown(input, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("textbox", { name: "Pfad" })).toBeNull());
    expect(bar()).toHaveAttribute("title", "/var/www/shop");

    fireEvent.click(screen.getByTestId("path-bar-edit"));
    const again = await screen.findByRole("textbox", { name: "Pfad" });
    fireEvent.change(again, { target: { value: "/etc" } });
    fireEvent.submit(again);
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", "/etc", null));
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/etc"));
  });

  it("a failed load keeps the input open with the typed text; Escape and blur restore the last loaded path", async () => {
    renderPanel();
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var/www/shop"));
    fireEvent.click(screen.getByTestId("path-bar-edit"));
    const input = await screen.findByRole("textbox", { name: "Pfad" });
    vi.mocked(sftpList).mockRejectedValueOnce(new Error("no such directory"));
    fireEvent.change(input, { target: { value: "/nope" } });
    fireEvent.submit(input);
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", "/nope", null));
    expect(await screen.findByText(/no such directory/)).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Pfad" })).toHaveValue("/nope");

    fireEvent.keyDown(screen.getByRole("textbox", { name: "Pfad" }), { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("textbox", { name: "Pfad" })).toBeNull());
    expect(bar()).toHaveAttribute("title", "/var/www/shop");

    fireEvent.click(screen.getByTestId("path-bar-edit"));
    const second = await screen.findByRole("textbox", { name: "Pfad" });
    vi.mocked(sftpList).mockRejectedValueOnce(new Error("no such directory"));
    fireEvent.change(second, { target: { value: "/nope2" } });
    fireEvent.submit(second);
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", "/nope2", null));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Pfad" })).toHaveValue("/nope2"));
    fireEvent.blur(screen.getByRole("textbox", { name: "Pfad" }));
    await waitFor(() => expect(screen.queryByRole("textbox", { name: "Pfad" })).toBeNull());
    expect(bar()).toHaveAttribute("title", "/var/www/shop");
  });

  it("hovering a symlink name shows name -> target (no inner name title hides it)", async () => {
    renderPanel();
    const name = (await screen.findAllByText("current"))[0];
    expect(name).not.toHaveAttribute("title");
    expect(name.closest("[title]")).toHaveAttribute("title", "current -> /var/www/shop/releases/42");
  });

  it("Ctrl+L inside the file browser switches to the input", async () => {
    renderPanel();
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var/www/shop"));
    fireEvent.keyDown(within(bar()).getByText("shop"), { key: "l", ctrlKey: true });
    const input = await screen.findByRole("textbox", { name: "Pfad" });
    await waitFor(() => expect(input).toHaveValue("/var/www/shop"));
  });

  it("Cmd+L (metaKey) inside the file browser switches to the input", async () => {
    renderPanel();
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var/www/shop"));
    fireEvent.keyDown(within(bar()).getByText("shop"), { key: "l", metaKey: true });
    const input = await screen.findByRole("textbox", { name: "Pfad" });
    await waitFor(() => expect(input).toHaveValue("/var/www/shop"));
  });

  it("clicking the last breadcrumb segment enters edit mode without loading", async () => {
    renderPanel();
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var/www/shop"));
    const listCalls = vi.mocked(sftpList).mock.calls.length;
    fireEvent.click(within(bar()).getByText("shop"));
    const input = await screen.findByRole("textbox", { name: "Pfad" });
    await waitFor(() => expect(input).toHaveValue("/var/www/shop"));
    expect(vi.mocked(sftpList).mock.calls.length).toBe(listCalls);
  });

  it("clicking a middle breadcrumb segment loads that path and stays in breadcrumb mode", async () => {
    renderPanel();
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var/www/shop"));
    fireEvent.click(within(bar()).getByText("var"));
    await waitFor(() => expect(sftpList).toHaveBeenLastCalledWith("session-1", "/var", null));
    await waitFor(() => expect(bar()).toHaveAttribute("title", "/var"));
    expect(screen.queryByRole("textbox", { name: "Pfad" })).toBeNull();
  });

  it("sortable headers: inactive ones show the faint chev-r marker, the active one a chevron", async () => {
    renderPanel();
    await screen.findByText(/current/);
    const icon = (column: string) =>
      screen.getByTestId(`column-sort-${column}`).querySelector("[data-icon]");
    const active = await waitFor(() => {
      const th = document.querySelector('thead th[aria-sort="ascending"], thead th[aria-sort="descending"]');
      expect(th).not.toBeNull();
      return th as Element;
    });
    const activeButton = active.querySelector('[data-testid^="column-sort-"]') as HTMLElement;
    const activeColumn = activeButton.getAttribute("data-testid")!.replace("column-sort-", "");
    expect(["chev-u", "chev-d"]).toContain(icon(activeColumn)?.getAttribute("data-icon"));
    const inactive = Array.from(document.querySelectorAll('[data-testid^="column-sort-"]'))
      .map((b) => b.getAttribute("data-testid")!.replace("column-sort-", ""))
      .filter((c) => c !== activeColumn);
    expect(inactive.length).toBeGreaterThan(0);
    for (const column of inactive) {
      expect(icon(column)).toHaveAttribute("data-icon", "chev-r");
      expect(icon(column)).toHaveClass("text-fg-faint");
    }
  });

  it("a symlink row shows the target inline, with the target in the title", async () => {
    renderPanel();
    const marker = await screen.findByText("→ /var/www/shop/releases/42");
    expect(marker.closest("[data-link-marker]")).toHaveAttribute("title", "/var/www/shop/releases/42");
  });

  it("the toolbar button opens the same columns menu and hiding a column persists", async () => {
    vi.mocked(sftpList).mockResolvedValue([link]);
    const { container } = renderPanel();
    await screen.findByText(/current/);
    // Wait for the persisted hidden columns to be loaded and applied before opening the menu.
    await waitFor(() => expect(loadHiddenFileColumns).toHaveBeenCalled());
    await act(async () => {});

    fireEvent.click(screen.getByRole("button", { name: "Spalten ein- oder ausblenden" }));
    const menu = await screen.findByRole("menu", { name: "Spalten ein- oder ausblenden" });
    const items = within(menu).getAllByRole("menuitemcheckbox");
    expect(items.map((i) => i.textContent)).toEqual(["Größe", "Rechte", "Geändert", "Besitzer"]);
    fireEvent.click(items[3]);
    await waitFor(() => expect(container.querySelectorAll("thead th").length).toBe(5));
    expect(saveHiddenFileColumns).toHaveBeenCalledWith(["owner"]);
  });
});
