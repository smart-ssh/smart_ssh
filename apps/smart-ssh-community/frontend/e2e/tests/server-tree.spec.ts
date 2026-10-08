// Case 6: the server tree in "Manage".
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { populated } from "../support/data";

/** Drags with real pointer events, in steps, like a user's mouse. */
async function dragOnto(page: Page, source: Locator, target: Locator) {
  const from = await source.boundingBox();
  const to = await target.boundingBox();
  expect(from && to, "drag source and target need a layout box").toBeTruthy();
  await page.mouse.move(from!.x + from!.width / 2, from!.y + from!.height / 2);
  await page.mouse.down();
  await page.mouse.move(to!.x + to!.width / 2, to!.y + to!.height / 2, { steps: 12 });
  await page.mouse.up();
}

/** The block of a folder in the tree: its row plus everything inside it. */
function folderBlock(page: Page, folderRow: Locator): Locator {
  return page.locator("[data-drop-target]").filter({ has: folderRow }).last();
}

test.describe("server tree", () => {
  test.beforeEach(async ({ app }) => {
    await app.launch(populated());
    await app.page.getByRole("button", { name: "Manage", exact: true }).click();
  });

  test("dragging a server onto a folder moves it there", async ({ app }) => {
    const page = app.page;
    const serverRow = page.getByRole("button", { name: "🖥️ standalone" });
    const labRow = page.getByRole("button", { name: "📁 Lab" });
    await expect(folderBlock(page, labRow)).not.toContainText("standalone");

    await dragOnto(page, serverRow, labRow);

    await expect.poll(() => app.calls("move_server_to_group")).toEqual([
      { cmd: "move_server_to_group", args: { id: "s-loose", groupId: "g-lab" } },
    ]);
    // The tree reloads and shows the server inside the folder.
    await expect(folderBlock(page, labRow).getByRole("button", { name: "🖥️ standalone" })).toBeVisible();
  });

  test("a new server created while a folder is selected starts in that folder", async ({ app }) => {
    const page = app.page;
    await page.getByRole("button", { name: "📁 Lab" }).click();
    await page.getByRole("button", { name: "+ Server", exact: true }).click();
    // The form's "Group" field (a select inside its label).
    const groupField = page
      .locator("label")
      .filter({ hasText: /^\s*Group/ })
      .locator("select");
    await expect(groupField).toHaveValue("g-lab");
  });
});
