// Issue #1422: drag and drop in the Connect server tree (flat rows, Spec 0103).
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { populated } from "../support/data";

/** The block of a folder in the tree: its row plus everything inside it. */
function folderBlock(page: Page, folderRow: Locator): Locator {
  return page.locator("[data-drop-target]").filter({ has: folderRow }).last();
}

async function drag(page: Page, from: Locator, to: Locator): Promise<void> {
  const a = await from.boundingBox();
  const b = await to.boundingBox();
  expect(a && b, "drag source and target need a layout box").toBeTruthy();
  await page.mouse.move(a!.x + a!.width / 2, a!.y + a!.height / 2);
  await page.mouse.down();
  await page.mouse.move(b!.x + b!.width / 2, b!.y + b!.height / 2, { steps: 12 });
  await page.mouse.up();
}

test.describe("Connect tree drag and drop", () => {
  test.beforeEach(async ({ app }, testInfo) => {
    test.skip(testInfo.project.name.includes("phone"), "pointer drag on the wide layout only");
    await app.launch(populated());
  });

  test("dragging a server onto a folder row moves it there", async ({ app }) => {
    const page = app.page;
    const serverRow = page.getByRole("button", { name: /^standalone\b/ });
    const labRow = page.getByRole("button", { name: /^Lab\b/ });
    await expect(serverRow).toBeVisible();
    await expect(folderBlock(page, labRow)).not.toContainText("standalone");

    await drag(page, serverRow, labRow);

    await expect.poll(() => app.calls("move_server_to_group")).toEqual([
      { cmd: "move_server_to_group", args: { id: "s-loose", groupId: "g-lab" } },
    ]);
    await expect(folderBlock(page, labRow).getByRole("button", { name: /^standalone\b/ })).toBeVisible();
  });

  test("dropping a folder into its own descendant changes nothing", async ({ app }) => {
    const page = app.page;
    const prodRow = page.getByRole("button", { name: /^Production\b/ });
    const webRow = page.getByRole("button", { name: /^Web\b/ });
    const prodBlock = folderBlock(page, prodRow);
    await expect(prodBlock.getByRole("button", { name: /^web-1\b/ })).toBeVisible();

    await drag(page, prodRow, webRow);

    // The backend rejects the cycle; the error is shown and the tree is unchanged.
    await expect(page.getByText("Cycle: the selected parent group is a subgroup of this group")).toBeVisible();
    await expect(prodBlock.getByRole("button", { name: /^web-1\b/ })).toBeVisible();
    await expect(prodBlock.getByRole("button", { name: /^Web\b/ })).toBeVisible();
    expect(await app.calls("move_group")).toEqual([
      { cmd: "move_group", args: { id: "g-prod", parentId: "g-web" } },
    ]);
  });

  test("dragging a folder onto another folder nests it there", async ({ app }) => {
    const page = app.page;
    const labRow = page.getByRole("button", { name: /^Lab\b/ });
    const webRow = page.getByRole("button", { name: /^Web\b/ });
    await drag(page, labRow, webRow);
    await expect.poll(() => app.calls("move_group")).toEqual([
      { cmd: "move_group", args: { id: "g-lab", parentId: "g-web" } },
    ]);
    await expect(folderBlock(page, webRow).getByRole("button", { name: /^Lab\b/ })).toBeVisible();
  });
});
