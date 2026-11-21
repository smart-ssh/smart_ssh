// Case 6: the server tree in "Manage".
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { captureCurrent, captureScreen } from "../support/screenshots";

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
    if (process.env.SCREENSHOT_SCHEME === "dark") await page.emulateMedia({ colorScheme: "dark" });
    const serverRow = page.getByRole("treeitem", { name: "standalone", exact: true });
    const labRow = page.getByRole("treeitem", { name: "Lab", exact: true });
    await expect(folderBlock(page, labRow)).not.toContainText("standalone");

    // Hold the pointer over the folder: the ghost shows the mono name and the drop hint.
    const from = await serverRow.boundingBox();
    const to = await labRow.boundingBox();
    expect(from && to, "drag source and target need a layout box").toBeTruthy();
    await page.mouse.move(from!.x + from!.width / 2, from!.y + from!.height / 2);
    await page.mouse.down();
    await page.mouse.move(to!.x + to!.width / 2, to!.y + to!.height / 2, { steps: 12 });
    const ghost = page.locator("div[aria-hidden='true'].pointer-events-none").filter({ hasText: "→ move into this group" });
    await expect(ghost).toBeVisible();
    await expect(ghost.locator("span.font-mono")).toHaveText("standalone");
    await expect(ghost.locator('svg[data-icon="move"]')).toBeVisible();
    await captureCurrent(page, `manage-drag-${process.env.SCREENSHOT_SCHEME ?? "light"}`);
    await page.mouse.up();

    await expect.poll(() => app.calls("move_server_to_group")).toEqual([
      { cmd: "move_server_to_group", args: { id: "s-loose", groupId: "g-lab" } },
    ]);
    // The tree reloads and shows the server inside the folder.
    await expect(folderBlock(page, labRow).getByRole("treeitem", { name: "standalone", exact: true })).toBeVisible();
  });

  test("a new server created while a folder is selected starts in that folder", async ({ app }) => {
    const page = app.page;
    await page.getByRole("treeitem", { name: "Lab", exact: true }).click();
    await page.getByRole("button", { name: "+ Server", exact: true }).click();
    // The form's "Group" field, found through its label.
    const groupField = page.getByLabel("Group", { exact: true });
    await expect(groupField).toHaveValue("g-lab");
  });

  test("a folder collapses and expands by chevron and by Left/Right, and its tree stays a tree", async ({ app }) => {
    const page = app.page;
    const lab = page.getByRole("treeitem", { name: "Lab", exact: true });
    await expect(lab).toHaveAttribute("aria-expanded", "true");
    const inside = folderBlock(page, lab).getByRole("treeitem").nth(1);
    await expect(inside).toBeVisible();

    await lab.getByTestId("group-chevron").click();
    await expect(lab).toHaveAttribute("aria-expanded", "false");
    await expect(inside).toBeHidden();

    await lab.focus();
    await page.keyboard.press("ArrowRight");
    await expect(lab).toHaveAttribute("aria-expanded", "true");
    await expect(inside).toBeVisible();
    await page.keyboard.press("ArrowLeft");
    await expect(lab).toHaveAttribute("aria-expanded", "false");
    await expect(lab).toBeFocused();
  });

  // Review screenshots (SCREENSHOT_LABEL=before|after, SCREENSHOT_SCHEME=dark).
  test("review screenshots of the Manage sidebar", async ({ app }) => {
    if (process.env.SCREENSHOT_SCHEME === "dark") await app.page.emulateMedia({ colorScheme: "dark" });
    await app.page.getByRole("treeitem", { name: "standalone", exact: true }).click();
    await captureScreen(app.page, `manage-sidebar-${process.env.SCREENSHOT_SCHEME ?? "light"}`);
  });
});
