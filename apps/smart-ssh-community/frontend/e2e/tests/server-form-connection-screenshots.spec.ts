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
