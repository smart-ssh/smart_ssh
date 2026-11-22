// Issue #1459: arrow-key navigation in the authentication radiogroup of the
// server form's Connection tab (native radio behaviour, needs a real browser).
import { expect, test } from "../support/app";
import { populated } from "../support/data";

test("ArrowRight / ArrowLeft move the selected authentication radio", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await page.getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("treeitem", { name: /^web-1\b/ }).click();
  await page.getByRole("tab", { name: "Connection", exact: true }).click();
  const group = page.getByRole("radiogroup").first();
  const checked = group.getByRole("radio", { checked: true });
  await expect(checked).toBeVisible();
  const value = () => checked.evaluate((el) => (el as HTMLInputElement).value);
  const start = await value();
  await checked.focus();
  await page.keyboard.press("ArrowRight");
  await expect(async () => expect(await value()).not.toBe(start)).toPass();
  await expect(checked).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(async () => expect(await value()).toBe(start)).toPass();
  await expect(checked).toBeFocused();
});
