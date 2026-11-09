// Issue #747 / Spec 0053, part 3: a sort change in one mounted file browser
// panel is applied to every other mounted panel immediately.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpList } from "../api/api";
import { loadFileManagerColumnWidths, loadFileManagerSort, saveFileManagerSort } from "../settings/layoutSettings";
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
};
const small = { ...base, name: "a.txt", path: "a.txt", size: 1 };
const big = { ...base, name: "b.txt", path: "b.txt", size: 999 };

const rowNames = (el: HTMLElement) =>
  Array.from(el.querySelectorAll("tbody tr td:first-child")).map((td) => td.textContent?.trim());

describe("FileBrowserPanel sort propagation (#747)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(loadFileManagerSort).mockResolvedValue(null);
    vi.mocked(sftpList).mockResolvedValue([small, big]);
  });

  it("a sort change in one mounted panel is applied to another mounted panel", async () => {
    const { container } = render(
      <I18nextProvider i18n={testI18n}>
        <div data-testid="panel-a">
          <FileBrowserPanel sessionId="session-1" isVisible={true} />
        </div>
        <div data-testid="panel-b">
          <FileBrowserPanel sessionId="session-2" isVisible={true} />
        </div>
      </I18nextProvider>,
    );
    const a = within(screen.getByTestId("panel-a"));
    const b = within(screen.getByTestId("panel-b"));
    await waitFor(() => expect(container.querySelectorAll("tbody tr")).toHaveLength(4));

    fireEvent.click(a.getByTestId("column-sort-size"));
    await waitFor(() =>
      expect(b.getByTestId("column-sort-size").closest("th")).toHaveAttribute("aria-sort", "ascending"),
    );
    expect(b.getByTestId("column-sort-name").closest("th")).toHaveAttribute("aria-sort", "none");
    expect(rowNames(screen.getByTestId("panel-b"))).toEqual(["📄 a.txt", "📄 b.txt"]);

    // Reverses from the shared state, not a stale local one.
    fireEvent.click(b.getByTestId("column-sort-size"));
    await waitFor(() =>
      expect(a.getByTestId("column-sort-size").closest("th")).toHaveAttribute("aria-sort", "descending"),
    );
    expect(rowNames(screen.getByTestId("panel-a"))).toEqual(["📄 b.txt", "📄 a.txt"]);
    expect(saveFileManagerSort).toHaveBeenLastCalledWith({ column: "size", direction: "desc" });
  });
});
