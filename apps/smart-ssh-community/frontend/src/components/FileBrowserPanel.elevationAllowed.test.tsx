// Issue #1194 / Spec 0067: the elevation controls are only offered when the
// server profile allows elevated rights (closed on every failure).
import { render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpElevationAllowed, sftpList, sftpStartDirectory } from "../api/api";
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
    readLocalTextPreview: vi.fn(),
    sftpStat: vi.fn(),
    sftpOpenForEditing: vi.fn(),
    localFileMtime: vi.fn(),
    closeEditSession: vi.fn(),
    readEditCopyText: vi.fn(),
    writeEditCopyText: vi.fn(),
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

const fileEntry: RemoteEntryDto = {
  name: "a.txt",
  path: "a.txt",
  isDir: false,
  size: 3,
  permissions: "rw-r--r--",
  modified: null,
  permissionsOctal: 0o644,
  uid: 1000,
  gid: 1000,
  owner: null,
  group: null,
};

async function renderPanel() {
  render(
    <I18nextProvider i18n={testI18n}>
      <FileBrowserPanel sessionId="session-1" isVisible={true} />
    </I18nextProvider>,
  );
  await screen.findByText(/a\.txt/);
}

describe("FileBrowserPanel elevation switch of the profile (issue #1194)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(sftpStartDirectory).mockResolvedValue({ path: ".", missingDirectory: null });
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
  });

  it("offers the elevation control when the profile allows it", async () => {
    vi.mocked(sftpElevationAllowed).mockResolvedValue(true);
    await renderPanel();
    expect(await screen.findByRole("button", { name: /Erhöhte Rechte/ })).toBeInTheDocument();
  });

  it.each([
    ["is off", () => vi.mocked(sftpElevationAllowed).mockResolvedValue(false)],
    ["cannot be read", () => vi.mocked(sftpElevationAllowed).mockRejectedValue("gone")],
  ])("hides the elevation control when the profile switch %s", async (_name, arrange) => {
    arrange();
    await renderPanel();
    await waitFor(() => expect(sftpElevationAllowed).toHaveBeenCalledWith("session-1"));
    expect(screen.queryByRole("button", { name: /Erhöhte Rechte/ })).not.toBeInTheDocument();
  });
});
