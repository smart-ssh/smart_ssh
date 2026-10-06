// Spec 0102: der Dateibrowser öffnet im Startverzeichnis der Sitzung,
// „Zum Startverzeichnis" kehrt dorthin zurück, „Aufwärts" funktioniert von
// dort, und ein fehlendes Verzeichnis führt ins Home plus einen Hinweis.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpList, sftpStartDirectory } from "../api";
import { testI18n } from "../testI18n";
import { showToast } from "../toastBus";
import type { RemoteEntryDto } from "../types";
import { FileBrowserPanel } from "./FileBrowserPanel";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  sftpList: vi.fn(),
  sftpStartDirectory: vi.fn(),
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
    group: null,
  };
}

function renderPanel() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <FileBrowserPanel sessionId="session-1" isVisible={true} />
    </I18nextProvider>,
  );
}

describe("FileBrowserPanel start directory (Spec 0102)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(sftpList).mockResolvedValue([dirEntry("/srv/app/sub")]);
  });

  it("opens in the start directory, returns there and goes up from there", async () => {
    vi.mocked(sftpStartDirectory).mockResolvedValue({
      path: "/srv/app",
      missingDirectory: null,
    });

    renderPanel();

    await waitFor(() => expect(sftpList).toHaveBeenCalledWith("session-1", "/srv/app", null));
    expect(sftpList).not.toHaveBeenCalledWith("session-1", ".", null);
    await screen.findByDisplayValue("/srv/app");

    fireEvent.click(screen.getByTitle("Übergeordnetes Verzeichnis"));
    await screen.findByDisplayValue("/srv");
    expect(sftpList).toHaveBeenLastCalledWith("session-1", "/srv", null);

    fireEvent.click(screen.getByTitle("Zum Startverzeichnis"));
    await screen.findByDisplayValue("/srv/app");
    expect(sftpList).toHaveBeenLastCalledWith("session-1", "/srv/app", null);
    expect(showToast).not.toHaveBeenCalled();
  });

  it("goes up from a home-relative start directory back to home", async () => {
    vi.mocked(sftpStartDirectory).mockResolvedValue({
      path: "./projects",
      missingDirectory: null,
    });

    renderPanel();
    await screen.findByDisplayValue("./projects");

    fireEvent.click(screen.getByTitle("Übergeordnetes Verzeichnis"));
    await screen.findByDisplayValue(".");
    expect(sftpList).toHaveBeenLastCalledWith("session-1", ".", null);
  });

  it("falls back to home and shows the notice when the directory is missing", async () => {
    vi.mocked(sftpStartDirectory).mockResolvedValue({
      path: ".",
      missingDirectory: "/srv/gone",
    });

    renderPanel();

    await waitFor(() => expect(sftpList).toHaveBeenCalledWith("session-1", ".", null));
    expect(showToast).toHaveBeenCalledTimes(1);
    expect(vi.mocked(showToast).mock.calls[0][0].message).toContain("/srv/gone");
  });

  it("shows no notice when the backend already handed it out", async () => {
    vi.mocked(sftpStartDirectory).mockResolvedValue({ path: ".", missingDirectory: null });

    renderPanel();

    await waitFor(() => expect(sftpList).toHaveBeenCalledWith("session-1", ".", null));
    expect(showToast).not.toHaveBeenCalled();
  });

  it("starts in home when the start directory query itself fails", async () => {
    vi.mocked(sftpStartDirectory).mockRejectedValue("session gone");

    renderPanel();

    await waitFor(() => expect(sftpList).toHaveBeenCalledWith("session-1", ".", null));
    expect(showToast).not.toHaveBeenCalled();
  });
});
