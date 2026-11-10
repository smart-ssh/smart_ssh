// Issue #1291: symbolic links in the file browser — rendering, sorting,
// navigation and the properties dialog for links to folders, links to files
// and broken links.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpList } from "../api/api";
import { loadFileManagerColumnWidths, loadFileManagerSort } from "../settings/layoutSettings";
import { testI18n } from "../testI18n";
import type { RemoteEntryDto } from "../api/types";
import { FileBrowserPanel } from "./FileBrowserPanel";

if (!Element.prototype.setPointerCapture) {
  Element.prototype.setPointerCapture = () => {};
  Element.prototype.releasePointerCapture = () => {};
}

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    sessionIsRoot: vi.fn(() => Promise.resolve(false)),
    sftpElevationDisable: vi.fn(() => Promise.resolve({ disabled: true, runningOperations: [] })),
    claimDroppedPaths: vi.fn(() => Promise.resolve([])),
    sftpElevationStatus: vi.fn(() => Promise.resolve(null)),
    clipboardHasFiles: vi.fn(() => Promise.resolve(false)),
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

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn(() => Promise.resolve(() => {})) }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn(),
  revealItemInDir: vi.fn(() => Promise.resolve()),
}));
vi.mock("../buses/toastBus", () => ({ showToast: vi.fn() }));
vi.mock("../settings/fileTypeSettings", () => ({
  loadFileTypeApps: vi.fn(() => Promise.resolve({})),
  appForFileName: () => null,
}));
vi.mock("../settings/layoutSettings", () => ({
  loadFileManagerColumnWidths: vi.fn(() => Promise.resolve({})),
  saveFileManagerColumnWidths: vi.fn(() => Promise.resolve()),
  loadHiddenFileColumns: vi.fn(() => Promise.resolve([])),
  saveHiddenFileColumns: vi.fn(() => Promise.resolve()),
  loadFileManagerSort: vi.fn(() => Promise.resolve(null)),
  saveFileManagerSort: vi.fn(() => Promise.resolve()),
}));

import { NarrowEntryList } from "./fileBrowser/NarrowEntryList";
import { isRealDir } from "../lib/remotePath";

const base: RemoteEntryDto = {
  name: "",
  path: "",
  isDir: false,
  size: 0,
  permissions: "rw-r--r--",
  modified: null,
  permissionsOctal: 0o644,
  uid: 1000,
  gid: 1000,
  owner: null,
  group: null,
  isSymlink: false,
  linkTarget: null,
  linkKind: null,
};
const MARKER = testI18n.t("fileBrowser.linkMarker");
const folder = { ...base, name: "zfolder", path: "/h/zfolder", isDir: true };
const file = { ...base, name: "afile", path: "/h/afile" };
const linkDir: RemoteEntryDto = {
  ...base,
  name: "link-dir",
  path: "/h/link-dir",
  isDir: true,
  isSymlink: true,
  linkTarget: "/srv/releases/42",
  linkKind: "dir",
};
const linkFile: RemoteEntryDto = {
  ...base,
  name: "link-file",
  path: "/h/link-file",
  isSymlink: true,
  linkTarget: "afile",
  linkKind: "file",
};
const dangling: RemoteEntryDto = {
  ...base,
  name: "dangling",
  path: "/h/dangling",
  isSymlink: true,
  linkTarget: null,
  linkKind: "broken",
};

describe("symbolic links in the file browser (#1291)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(loadFileManagerSort).mockResolvedValue(null);
    vi.mocked(sftpList).mockResolvedValue([file, linkFile, dangling, linkDir, folder]);
  });

  it("wide table: a link to a folder sorts with the folders and opens like one; the other links are not navigable", async () => {
    render(
      <I18nextProvider i18n={testI18n}>
        <FileBrowserPanel sessionId="s1" isVisible={true} />
      </I18nextProvider>,
    );
    const rows = await screen.findAllByRole("row");
    expect(rows.length).toBeGreaterThan(5);
    const names = Array.from(document.querySelectorAll("tbody tr td:first-child")).map((td) =>
      td.textContent?.replace(/\s+/g, " ").trim(),
    );
    // Folders first (link-dir < zfolder), then files by name.
    expect(names.map((n) => n?.split(" ")[1])).toEqual(["link-dir", "zfolder", "afile", "dangling", "link-file"]);
    expect(names[0]).toContain("📁");
    expect(names[2]).not.toContain("↪");
    expect(names[4]).toContain("📄");
    // Every link has the accessible marker; ordinary entries have none.
    expect(screen.getAllByText(MARKER)).toHaveLength(3);
    // Hover shows the target; an unreadable target is omitted.
    expect(screen.getAllByTitle("link-dir -> /srv/releases/42").length).toBeGreaterThan(0);
    expect(screen.getAllByTitle("link-file -> afile").length).toBeGreaterThan(0);
    expect(screen.getAllByTitle("dangling").length).toBeGreaterThan(0);
    // Only folder-like entries are buttons.
    expect(screen.getByRole("button", { name: /^📁 link-dir/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^📄 link-file/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /^⚠️ dangling/ })).toBeNull();
    // The broken link is de-emphasised.
    expect(screen.getAllByTitle("dangling")[0].closest("span.opacity-50")).not.toBeNull();

    vi.mocked(sftpList).mockClear();
    fireEvent.click(screen.getByRole("button", { name: /^📁 link-dir/ }));
    await waitFor(() => expect(sftpList).toHaveBeenCalledWith("s1", "/h/link-dir", null));
  });

  it("narrow list: same three kinds, navigation uses the link path", () => {
    const load = vi.fn();
    render(
      <I18nextProvider i18n={testI18n}>
        <NarrowEntryList
          entries={[linkDir, linkFile, dangling]}
          openMenu={null}
          setOpenMenu={() => {}}
          setDirMenu={() => {}}
          load={load}
          renderEntryMenu={() => null}
        />
      </I18nextProvider>,
    );
    expect(screen.getAllByText(MARKER)).toHaveLength(3);
    expect(screen.getAllByTitle("link-dir -> /srv/releases/42").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: /^📄 link-file/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /^⚠️ dangling/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^📁 link-dir/ }));
    expect(load).toHaveBeenCalledWith("/h/link-dir");
  });

  it("truncates a very long target in the tooltip", () => {
    const long = { ...linkFile, linkTarget: "x".repeat(500) };
    render(
      <I18nextProvider i18n={testI18n}>
        <NarrowEntryList
          entries={[long]}
          openMenu={null}
          setOpenMenu={() => {}}
          setDirMenu={() => {}}
          load={() => {}}
          renderEntryMenu={() => null}
        />
      </I18nextProvider>,
    );
    const title = screen.getByText(MARKER).closest("[data-link-marker]")?.getAttribute("title") ?? "";
    expect(title.length).toBeLessThan(260);
  });

  it("a link to a folder is not a real folder for recursive operations", () => {
    expect(isRealDir(linkDir)).toBe(false);
    expect(isRealDir(folder)).toBe(true);
    expect(isRealDir(linkFile)).toBe(false);
  });
});
