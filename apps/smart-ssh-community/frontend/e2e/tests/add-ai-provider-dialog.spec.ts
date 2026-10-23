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
