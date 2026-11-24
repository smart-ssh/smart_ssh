// Issue #1385: screenshots of the move-to dialog (reachable at phone width).
import { expect, test } from "../support/app";
import { ACKNOWLEDGED, populated } from "../support/data";
import { captureScreen } from "../support/screenshots";

test("move-to dialog is captured", async ({ app }) => {
  await app.launch({ ...populated(), settings: { ...ACKNOWLEDGED } });
  const page = app.page;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Open navigation menu", exact: true }).click();
  await page.getByRole("dialog", { name: "Navigation menu" }).getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("button", { name: "Move “web-1” to…", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await captureScreen(page, "move-to-dialog");
});

test("import dialog with a conflict row and rename input is captured", async ({ app }) => {
  const entry = (index: number, name: string, conflict: unknown, jumpHost: unknown) => ({
    index, name, group: 0,
    host: { value: `host-${index}.example.test`, origin: null },
    port: { value: 22, origin: null },
    username: { value: "deploy", origin: null },
    tags: [], identityFile: null, jumpHost, conflict,
  });
  const preview = {
    groups: [{ name: "config", parent: null, sourcePath: "/home/me/.ssh/config" }],
    entries: [
      entry(0, "web-1", null, null),
      entry(1, "db-primary", { kind: "name", existingName: "db-primary" }, { name: "web-1", existing: false }),
    ],
    files: [{ path: "/home/me/.ssh/config", depth: 0, status: "read" }],
    skipped: [],
    identityFilePaths: [],
  };
  await app.launch({ ...populated(), settings: { ...ACKNOWLEDGED }, commands: { preview_ssh_config_import: { result: preview } } });
  const page = app.page;
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByRole("button", { name: "Manage", exact: true }).first().click();
  await page.getByRole("button", { name: "Import …", exact: true }).click();
  await expect(page.getByPlaceholder("new name")).toBeVisible();
  await captureScreen(page, "ssh-config-import-conflict");
});
