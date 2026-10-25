// Issue #679 / Spec 0114: auf Android kein Drag&Drop, Download über den
// Speichern-Dialog, „Lokal öffnen" im App-Editor statt in einem externen Programm.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { openPath } from "@tauri-apps/plugin-opener";

import {
  localFileMtime,
  readEditCopyText,
  sftpDownload,
  sftpDownloadDefault,
  sftpList,
  sftpOpenForEditing,
  sftpStartDirectory,
} from "../api";
import { testI18n } from "../testI18n";
import type { RemoteEntryDto } from "../types";
import { FileBrowserPanel } from "./FileBrowserPanel";

const dragDropRegistrations = vi.hoisted(() => vi.fn(() => Promise.resolve(() => {})));

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
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

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: dragDropRegistrations,
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

async function openActions() {
  render(
    <I18nextProvider i18n={testI18n}>
      <FileBrowserPanel sessionId="session-1" isVisible={true} />
    </I18nextProvider>,
  );
  await screen.findByText(/a\.txt/);
  fireEvent.click(screen.getByRole("button", { name: /^(Aktionen|Actions): / }));
}

describe("FileBrowserPanel on Android (issue #679)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(
      "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36",
    );
    vi.mocked(sftpStartDirectory).mockResolvedValue({ path: ".", missingDirectory: null });
    vi.mocked(sftpList).mockResolvedValue([fileEntry]);
  });

  it("registers no OS drag and drop listener", async () => {
    await openActions();
    expect(dragDropRegistrations).not.toHaveBeenCalled();
  });

  it("downloads a file through the save picker, not the default folder", async () => {
    vi.mocked(sftpDownload).mockResolvedValue(null);
    await openActions();
    fireEvent.click(screen.getByText("Herunterladen"));
    expect(sftpDownload).toHaveBeenCalledWith("session-1", "a.txt", null);
    expect(sftpDownloadDefault).not.toHaveBeenCalled();
  });

  it("opens a file locally in the in-app editor, not an external program", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/a.txt",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");
    vi.mocked(readEditCopyText).mockResolvedValue("abc");
    await openActions();
    fireEvent.click(screen.getByText("Lokal öffnen…"));
    await waitFor(() => expect(readEditCopyText).toHaveBeenCalledWith("session-1", "/tmp/edit/a.txt"));
    expect(openPath).not.toHaveBeenCalled();
  });
});
