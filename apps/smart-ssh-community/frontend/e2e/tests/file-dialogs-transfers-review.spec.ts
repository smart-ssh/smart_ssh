// Issue #1386: screenshots of the transfer and elevation dialogs of the file
// browser (upload conflict, folder confirm/summary, elevation failure, stop prompt).
import type { AppDriver } from "../support/app";
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { captureScreen } from "../support/screenshots";
import { openSession } from "../support/session";

const ops = [
  { id: "1", kind: "download", path: "/var/backups/db.tar.gz", bytesDone: 52_428_800, bytesTotal: 209_715_200, filesDone: null, filesTotal: null },
  { id: "2", kind: "delete", path: "/var/log/old", bytesDone: null, bytesTotal: null, filesDone: 12, filesTotal: 40 },
];

async function openFiles(app: AppDriver, commands: Record<string, { result: unknown } | { error: unknown }>) {
  await app.launch({ ...populated(), commands: { sftp_list: { result: [] }, ...commands } });
  await openSession(app, "db-primary");
  await app.page.getByRole("button", { name: /^Files/ }).click();
}

test("upload conflict dialog is captured", async ({ app }) => {
  await openFiles(app, {
    pick_upload_files: { result: ["/home/me/app.bin"] },
    sftp_exists: { result: true },
    read_local_text_preview: { result: { text: null, size: 2_097_152 } },
    sftp_read_text: { error: { code: "binary", message: "binary file" } },
  });
  await app.page.getByRole("button", { name: "Upload", exact: true }).click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "file-dialog-upload-conflict");
});

test("folder confirm dialog is captured", async ({ app }) => {
  await openFiles(app, {
    pick_upload_folder: { result: "/home/me/site" },
    sftp_upload_folder_preview: {
      result: { folderName: "site", fileCount: 14, folderCount: 3, overwrites: ["/srv/site/index.html", "/srv/site/css/app.css"], skipped: [] },
    },
  });
  await app.page.getByRole("button", { name: "Upload folder", exact: true }).click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "file-dialog-folder-confirm");
});

test("folder summary dialog is captured", async ({ app }) => {
  await openFiles(app, {
    pick_upload_folder: { result: "/home/me/site" },
    sftp_upload_folder_preview: { result: { folderName: "site", fileCount: 14, folderCount: 3, overwrites: [], skipped: [] } },
    sftp_upload_folder: {
      result: {
        folderName: "site", filesUploaded: 12, foldersCreated: 3, notAttempted: 0,
        skipped: [{ path: "site/current", reason: "symlink" }],
        failed: [{ path: "site/secret.key", error: "Permission denied" }],
      },
    },
  });
  await app.page.getByRole("button", { name: "Upload folder", exact: true }).click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "file-dialog-folder-summary");
});

test("elevation failure dialog is captured", async ({ app }) => {
  await openFiles(app, {
    sftp_elevation_enable: {
      result: {
        active: false, targetUser: "root", sftpServerPath: null, warning: null, runningOperations: [],
        serverLogging: false, sudoAuth: null, failure: { kind: "notAllowed", sudoersLine: "deploy ALL=(ALL) NOPASSWD: ALL", detail: null },
      },
    },
  });
  await app.page.getByRole("button", { name: /Elevated rights/ }).click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "file-dialog-elevation-failure");
});

test("stop prompt dialog is captured", async ({ app }) => {
  await openFiles(app, {
    sftp_elevation_enable: {
      result: {
        active: true, targetUser: "root", sftpServerPath: null, warning: null, runningOperations: ops,
        serverLogging: false, sudoAuth: null, failure: null,
      },
    },
  });
  await app.page.getByRole("button", { name: /Elevated rights/ }).click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "file-dialog-stop-prompt");
});

test("local edit upload dialog is captured", async ({ app }) => {
  const file = {
    name: "app.bin", path: "/home/deploy/app.bin", isDir: false, size: 1_048_576, permissions: "rw-r--r--",
    modified: "2026-01-15T10:30:00Z", permissionsOctal: 0o644, uid: 1000, gid: 1000,
    owner: "deploy", group: "deploy", isSymlink: false, linkTarget: null, linkKind: null,
  };
  await openFiles(app, {
    sftp_list: { result: [file] },
    sftp_open_for_editing: { result: { localPath: "/tmp/edit/app.bin", remoteModified: "2026-01-15T10:30:00Z" } },
    local_file_mtime: { result: "2026-01-15T11:00:00Z" },
    read_local_text_preview: { result: { text: null, size: 1_179_648 } },
    sftp_read_text: { error: { code: "binary", message: "binary file" } },
    sftp_stat: { result: file },
    "plugin:opener|open_path": { result: null },
  });
  await app.page.getByRole("button", { name: /^Actions: app.bin$/ }).click();
  await app.page.getByRole("menuitem", { name: "Open locally…", exact: true }).click();
  await app.setCommand("local_file_mtime", { result: "2026-01-15T12:00:00Z" });
  await expect(app.page.getByText(/was changed locally/)).toBeVisible({ timeout: 10_000 });
  await app.page.getByRole("button", { name: "Upload", exact: true }).last().click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "file-dialog-local-edit-upload");
});
