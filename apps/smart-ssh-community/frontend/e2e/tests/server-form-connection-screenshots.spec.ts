// Issue #1380: review screenshots of the server form's Connection tab
// (SCREENSHOT_LABEL=before|after, SCREENSHOT_SCHEME=dark) at every screenshot size.
import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { failingStepLog, populated, server } from "../support/data";
import { captureCurrent, captureScreen } from "../support/screenshots";

test("Connection tab screenshots", async ({ app }) => {
  await app.launch(populated());
  if (process.env.SCREENSHOT_SCHEME === "dark") await app.page.emulateMedia({ colorScheme: "dark" });
  await app.page.getByRole("button", { name: "Manage", exact: true }).click();
  await app.page.getByRole("treeitem", { name: /^web-1\b/ }).click();
  await app.page.getByRole("tab", { name: "Connection", exact: true }).click();
  await expect(app.page.getByRole("button", { name: "Test Connection", exact: true })).toBeVisible();
  // Review: every authentication label is fully readable (no clipped or overlapping text).
  const group = app.page.getByRole("radiogroup").first();
  for (const [width, height] of [[1440, 900], [800, 560], [390, 844]]) {
    await app.page.setViewportSize({ width, height });
    await expect(async () => {
      const clipped = await group.locator("label").evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth).map((e) => e.textContent));
      expect(clipped).toEqual([]);
    }).toPass();
  }
  await captureScreen(app.page, "server-form-connection");
});

// Review: states with a key-file server (findings), a stored sudo password and a
// successful test ("OK · n ms" badge), plus the two dialogs.
// Review: the new states are captured at 1440 px and 390 px.
async function captureAt1440And390(page: Page, name: string) {
  const original = page.viewportSize();
  try {
    for (const [width, height] of [[1440, 900], [390, 844]]) {
      await page.setViewportSize({ width, height });
      await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
      await captureCurrent(page, name);
    }
  } finally {
    if (original) await page.setViewportSize(original);
  }
}

test("Connection tab key-file states screenshots", async ({ app }) => {
  const base = populated();
  const steps = failingStepLog(1).map((r) => (r.status.state === "failed" ? { ...r, status: { state: "ok" as const } } : r));
  await app.launch({
    ...base,
    servers: [
      ...(base.servers ?? []),
      server({ id: "key-1", name: "key-1", authKind: "identity_file", identityFilePath: "/home/deploy/.ssh/id_ed25519", hasSudoPassword: true }),
    ],
    commands: {
      inspect_key_file: { result: { exists: true, permissionsTooOpen: true, validKey: true, encrypted: true, problem: null } },
      test_connection: { result: { kind: "success", steps } },
    },
  });
  if (process.env.SCREENSHOT_SCHEME === "dark") await app.page.emulateMedia({ colorScheme: "dark" });
  const page = app.page;
  await page.getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("treeitem", { name: /^key-1\b/ }).click();
  await page.getByRole("tab", { name: "Connection", exact: true }).click();
  await expect(page.getByRole("radio", { name: "Key file" })).toBeChecked();
  await page.getByRole("button", { name: "Test Connection", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByTestId("test-connection-result")).toContainText(/OK · \d+ ms/);
  await expect(dialog).toBeVisible();
  await captureAt1440And390(page, "server-form-test-dialog");
  const close = dialog.getByRole("button", { name: "Close", exact: true });
  await expect(close).toBeVisible();
  await expect(close).toBeEnabled();
  await close.click();
  await expect(dialog).toBeHidden();
  const log = page.getByTestId("connect-step-log");
  await log.getByText("Details", { exact: true }).click();
  await expect(log).toHaveAttribute("open");
  await captureAt1440And390(page, "server-form-connection-keyfile");
});
