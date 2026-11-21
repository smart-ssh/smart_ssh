// Issue #1386: screenshots of the file browser dialogs (delete folder, properties, chmod, chown, rename, new folder).
import type { RemoteEntryDto } from "../../src/api/types";
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { captureScreen } from "../support/screenshots";
import { openSession } from "../support/session";

const dir: RemoteEntryDto = {
  name: "logs", path: "/home/deploy/logs", isDir: true, size: 0, permissions: "rwxr-xr-x",
  modified: "2026-01-15T10:30:00Z", permissionsOctal: 0o755, uid: 1000, gid: 1000,
  owner: "deploy", group: "deploy", isSymlink: false, linkTarget: null, linkKind: null,
};

for (const item of ["Delete", "Properties", "Edit permissions…", "Change owner…", "Rename"]) {
  test(`file dialog ${item} is captured`, async ({ app }) => {
    await app.launch({
      ...populated(),
      commands: { sftp_list: { result: [dir] }, sftp_delete_preview: { result: { fileCount: 12, dirCount: 3 } } },
    });
    await openSession(app, "db-primary");
    await app.page.getByRole("button", { name: /^Files/ }).click();
    await app.page.getByRole("button", { name: /^Actions: logs$/ }).click();
    await app.page.getByRole("menuitem", { name: item, exact: true }).click();
    await expect(app.page.getByRole("dialog")).toBeVisible();
    await captureScreen(app.page, `file-dialog-${item.split(/\W/)[0].toLowerCase()}`);
  });
}

test("file dialog New folder is captured", async ({ app }) => {
  await app.launch({ ...populated(), commands: { sftp_list: { result: [dir] } } });
  await openSession(app, "db-primary");
  await app.page.getByRole("button", { name: /^Files/ }).click();
  await expect(app.page.getByRole("button", { name: /^Actions: logs$/ })).toBeVisible();
  const scroller = app.page.locator("table").locator("xpath=..");
  const box = (await scroller.boundingBox())!;
  await scroller.click({ button: "right", position: { x: box.width - 4, y: box.height - 4 } });
  await app.page.getByRole("menuitem", { name: "New folder", exact: true }).click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "file-dialog-newfolder");
});
