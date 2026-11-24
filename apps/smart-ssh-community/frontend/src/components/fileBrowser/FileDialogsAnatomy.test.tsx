// Issue #1386: file dialogs share one anatomy — header icon, path in its own
// mono paragraph, counts/sizes as label/value rows, permission bits in a mono grid.
import { render, screen, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import type { FolderUploadSummaryDto, RemoteEntryDto, RunningOperationDto } from "../../api/types";
import { formatBytes } from "../../lib/format";
import { testI18n } from "../../testI18n";
import { ChmodDialog } from "./ChmodDialog";
import { EditUploadDialog } from "./EditUploadDialog";
import { FilePropertiesDialog } from "./FilePropertiesDialog";
import { FolderSummaryDialog } from "./FolderSummaryDialog";
import { StopPromptDialog } from "./StopPromptDialog";
import { UploadConflictDialog } from "./UploadConflictDialog";

const entry: RemoteEntryDto = {
  name: "logs", path: "/home/deploy/logs", isDir: true, size: 0, permissions: "rwxr-xr-x",
  modified: "2026-01-15T10:30:00Z", permissionsOctal: 0o755, uid: 1000, gid: 1000,
  owner: "deploy", group: "deploy", isSymlink: false, linkTarget: null, linkKind: null,
};

const fmt = formatBytes;
const L = (key: string) => testI18n.t(key as never) as string;

function wrap(ui: React.ReactNode) {
  return render(<I18nextProvider i18n={testI18n}>{ui}</I18nextProvider>);
}

/** The value next to a row label inside the dialog. */
function rowValue(dialog: HTMLElement, label: string): string {
  const dt = within(dialog).getByText(label, { selector: "dt" });
  return dt.parentElement!.querySelector("dd")!.textContent ?? "";
}

function headerIconCount(dialog: HTMLElement): number {
  return dialog.querySelectorAll('span[aria-hidden="true"] svg').length;
}

describe("file dialog anatomy (#1386)", () => {
  it("properties: header icon and rows", () => {
    wrap(<FilePropertiesDialog entry={entry} onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(headerIconCount(dialog)).toBeGreaterThan(0);
    expect(within(dialog).getByText("/home/deploy/logs").className).toContain("font-mono");
  });

  it("chmod: header icon, path in its own mono paragraph, mono permission grid", () => {
    wrap(<ChmodDialog entry={entry} modeNote={null} onCancel={vi.fn()} onConfirm={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(headerIconCount(dialog)).toBeGreaterThan(0);
    const path = within(dialog).getByText("/home/deploy/logs");
    expect(path.tagName).toBe("P");
    expect(path.className).toContain("font-mono");
    expect(within(dialog).getByTestId("chmod-grid").className).toContain("font-mono");
  });

  it("upload conflict: path in own mono paragraph, sizes as rows", () => {
    wrap(
      <UploadConflictDialog
        conflict={{
          localPath: "/tmp/a.bin",
          remotePath: "/srv/a.bin",
          localPreview: { text: null, size: 2048 },
          remoteText: null,
          remoteReadFailure: null,
          remoteSize: 1024,
        }}
        elevated={false}
        elevatedUser={null}
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog");
    expect(headerIconCount(dialog)).toBeGreaterThan(0);
    const path = within(dialog).getByText("/srv/a.bin");
    expect(path.tagName).toBe("P");
    expect(path.className).toContain("font-mono");
    expect(rowValue(dialog, L("fileBrowser.uploadConflict.currentSize"))).toBe(fmt(1024));
    expect(rowValue(dialog, L("fileBrowser.uploadConflict.newSize"))).toBe(fmt(2048));
  });

  it("local edit upload: new size as a row", () => {
    wrap(
      <EditUploadDialog
        offer={{
          localText: null,
          localSize: 3072,
          remoteText: null,
          remoteReadFailure: null,
          remoteChangedSinceDownload: false,
        } as never}
        uploadAs={null}
        sessionElevatedUser={null}
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );
    const dialog = screen.getByRole("dialog");
    expect(headerIconCount(dialog)).toBeGreaterThan(0);
    expect(rowValue(dialog, L("fileBrowser.uploadConflict.newSize"))).toBe(fmt(3072));
  });

  it("folder summary: uploaded, skipped and failed counts as rows", () => {
    const summary: FolderUploadSummaryDto = {
      folderName: "site",
      filesUploaded: 7,
      foldersCreated: 1,
      skipped: [{ path: "a/link", reason: "symlink" }],
      failed: [{ path: "b/x", error: "denied" }, { path: "b/y", error: "denied" }],
      notAttempted: 0,
    } as FolderUploadSummaryDto;
    wrap(<FolderSummaryDialog folderSummary={summary} onClose={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(headerIconCount(dialog)).toBeGreaterThan(0);
    expect(rowValue(dialog, L("fileBrowser.folderUpload.uploaded"))).toBe("7");
    expect(within(dialog).getAllByText(L("fileBrowser.folderUpload.skipped"), { selector: "dt" })).toHaveLength(1);
    expect(rowValue(dialog, L("fileBrowser.folderUpload.skipped"))).toBe("1");
    expect(rowValue(dialog, L("fileBrowser.folderUpload.failed"))).toBe("2");
  });

  it("stop prompt: progress as a row, none without progress data", () => {
    const ops: RunningOperationDto[] = [
      { id: "1", kind: "download", path: "/srv/big.iso", bytesDone: 1024, bytesTotal: 2048, filesDone: 1, filesTotal: 4 },
      { id: "2", kind: "delete", path: "/srv/old", bytesDone: null, bytesTotal: null, filesDone: 2, filesTotal: 5 },
      { id: "3", kind: "chmod", path: "/srv/x", bytesDone: null, bytesTotal: null, filesDone: null, filesTotal: null },
    ];
    wrap(<StopPromptDialog operations={ops} onCancel={vi.fn()} onConfirm={vi.fn()} />);
    const dialog = screen.getByRole("dialog");
    expect(headerIconCount(dialog)).toBeGreaterThan(0);
    const values = within(dialog).getAllByText(L("fileBrowser.stopProgress"), { selector: "dt" }).map((dt) => dt.parentElement!.querySelector("dd")!.textContent);
    expect(values).toEqual([`${fmt(1024)} / ${fmt(2048)} — ${testI18n.t("fileBrowser.transferFiles", { done: 1, total: 4 })}`, testI18n.t("fileBrowser.entriesProgress", { done: 2, total: 5 })]);
  });
});
