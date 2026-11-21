// Issue #1380: review screenshots of the server form's Connection tab
// (SCREENSHOT_LABEL=before|after, SCREENSHOT_SCHEME=dark) at every screenshot size.
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { captureScreen } from "../support/screenshots";

test("Connection tab screenshots", async ({ app }) => {
  await app.launch(populated());
  if (process.env.SCREENSHOT_SCHEME === "dark") await app.page.emulateMedia({ colorScheme: "dark" });
  await app.page.getByRole("button", { name: "Manage", exact: true }).click();
  await app.page.getByRole("treeitem", { name: /^web-1\b/ }).click();
  await app.page.getByRole("tab", { name: "Connection", exact: true }).click();
  await expect(app.page.getByRole("button", { name: "Test Connection", exact: true })).toBeVisible();
  // Review: the segmented control stays on one row (no wrapped labels).
  const group = app.page.getByRole("radiogroup").first();
  const rows = await group.locator("label").evaluateAll((els) => new Set(els.map((e) => Math.round(e.getBoundingClientRect().top))).size);
  expect(rows).toBe(1);
  await captureScreen(app.page, "server-form-connection");
});
