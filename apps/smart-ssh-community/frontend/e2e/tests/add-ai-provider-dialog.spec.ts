// Issue #603: the add-provider form is a dialog over the Settings modal.
// Title and actions stay usable, only the body scrolls, focus returns.
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { expectOnlyContentScrolls, expectUsable, scrollContainerWithin } from "../support/layout";

test("add provider dialog: title and actions usable, body scrolls, focus returns", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "AI Provider", exact: true }).click();
  const addButton = page.getByRole("button", { name: "Add Provider", exact: true });
  await addButton.click();

  const dialog = page.getByRole("dialog", { name: "Add Provider" });
  await expect(dialog).toBeVisible();
  // Grow the body so it overflows in every viewport.
  await dialog.getByRole("button", { name: "Advanced" }).click();
  for (let i = 0; i < 8; i++) {
    await dialog.getByRole("button", { name: "+ Header", exact: true }).click();
  }

  const title = dialog.getByRole("heading", { name: "Add Provider" });
  const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
  const add = dialog.getByRole("button", { name: "Add", exact: true });
  await expectUsable(title);
  await expectUsable(cancel);
  await expectUsable(add);
  await expectOnlyContentScrolls(await scrollContainerWithin(dialog, dialog.getByLabel("Name", { exact: true })), [title, cancel, add]);
  await expectUsable(add);

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("button", { name: "AI Provider", exact: true })).toBeVisible();
  await expect(addButton).toBeFocused();
});

// Issue #1377: the add action sits in the section header and the dialog fields use `Field`.
test("add provider: header button is reachable, dialog fields have accessible names", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "AI Provider", exact: true }).click();
  const addButton = page.getByRole("button", { name: "Add Provider", exact: true });
  await expect(addButton).toHaveCount(1);
  await addButton.focus();
  await expect(addButton).toBeFocused();
  await page.keyboard.press("Enter");

  const dialog = page.getByRole("dialog", { name: "Add Provider" });
  await expect(dialog).toBeVisible();
  for (const name of ["Name", "API Key"]) {
    await expect(dialog.getByLabel(name, { exact: false }).first()).toBeVisible();
  }
  await expect(dialog.getByRole("combobox").first()).toBeVisible();
  await dialog.getByRole("button", { name: /Advanced/ }).click();
  await expect(dialog.getByLabel(/Max. response length/).first()).toBeVisible();
});
