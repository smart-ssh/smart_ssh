// UI review of the file browser (issue #384): keyboard order with visible
// focus, dialogs close with Escape and give focus back, axe scans, and no
// clipped text with long names at both window sizes.
import type { Locator, Page } from "@playwright/test";
import type { RemoteEntryDto } from "../../src/types";
import { expect, test, type AppDriver } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { populated } from "../support/data";
import { captureScreen } from "../support/screenshots";
import { openSession } from "../support/session";

const LONG_NAME = `${"very-long-directory-name-".repeat(6)}end.log`;

function entry(name: string, isDir = false): RemoteEntryDto {
  return {
    name,
    path: `/home/deploy/${name}`,
    isDir,
    size: isDir ? 0 : 2048,
    permissions: isDir ? "rwxr-xr-x" : "rw-r--r--",
    modified: "2026-01-15T10:30:00Z",
    permissionsOctal: isDir ? 0o755 : 0o644,
    uid: 1000,
    gid: 1000,
    owner: "deploy",
    group: "deploy",
  };
}

const ENTRIES = [entry("logs", true), entry("notes.txt"), entry(LONG_NAME)];

async function openFileBrowser(app: AppDriver, entries: RemoteEntryDto[] = ENTRIES) {
  await app.launch({ ...populated(), commands: { sftp_list: { result: entries } } });
  await openSession(app, "db-primary");
  await app.page.getByRole("button", { name: /^Files/ }).click();
  await expect(app.page.getByText("notes.txt")).toBeVisible();
}

function rowMenu(page: Page, name: string): Locator {
  return page.getByRole("button", { name: new RegExp(`^Actions: ${name}$`) });
}

async function focusRing(el: Locator): Promise<boolean> {
  return el.evaluate((node) => {
    const s = getComputedStyle(node);
    return s.boxShadow !== "none" || (s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0);
  });
}

test.describe("file browser", () => {
  test("axe scan: list, menu and dialogs", async ({ app }) => {
    test.setTimeout(90_000);
    await openFileBrowser(app);
    const page = app.page;
    await expectNoSeriousA11yViolations(page, "file browser");

    await rowMenu(page, "notes.txt").click();
    await expectNoSeriousA11yViolations(page, "file browser: entry menu");
    await page.getByRole("button", { name: "Properties", exact: true }).click();
    await expectNoSeriousA11yViolations(page, "file browser: properties");
    await page.keyboard.press("Escape");

    await rowMenu(page, "notes.txt").click();
    await page.getByRole("button", { name: /^Edit permissions|^Permissions/ }).click();
    await expectNoSeriousA11yViolations(page, "file browser: permissions");
    await page.keyboard.press("Escape");

    await rowMenu(page, "notes.txt").click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expectNoSeriousA11yViolations(page, "file browser: delete");
  });

  test("Tab follows the visual order and every stop shows a focus indicator", async ({ app }) => {
    await openFileBrowser(app);
    const page = app.page;
    // Focus is on the "Files" tab after opening it; every further stop is
    // reached with the keyboard, so :focus-visible applies.
    const seen: { x: number; y: number }[] = [];
    for (let i = 0; i < 9; i++) {
      await page.keyboard.press("Tab");
      const active = page.locator(":focus");
      expect(await focusRing(active), `focus indicator on stop ${i}`).toBe(true);
      const box = (await active.boundingBox())!;
      seen.push({ x: box.x, y: box.y });
    }
    // Reading order: rows top to bottom, within a row left to right.
    for (let i = 1; i < seen.length; i++) {
      const prev = seen[i - 1];
      const cur = seen[i];
      const sameRow = Math.abs(cur.y - prev.y) < 8;
      expect(
        cur.y > prev.y - 8 && (!sameRow || cur.x >= prev.x - 1),
        `stop ${i} (${JSON.stringify(cur)}) comes before stop ${i - 1} (${JSON.stringify(prev)})`,
      ).toBe(true);
    }
  });

  test("dialogs close with Escape and return focus to their trigger", async ({ app }) => {
    await openFileBrowser(app);
    const page = app.page;
    const cases = [{ item: "Properties" }, { item: "Delete" }, { item: "Rename" }];
    for (const c of cases) {
      const trigger = rowMenu(page, "notes.txt");
      await trigger.focus();
      await page.keyboard.press("Enter");
      await page.getByRole("button", { name: c.item, exact: true }).click();
      const dialog = page.getByRole("dialog");
      await expect(dialog).toBeVisible();
      await page.keyboard.press("Escape");
      await expect(dialog).toBeHidden();
      await expect(trigger).toBeFocused();
    }
    const newFolder = page.getByRole("button", { name: "+ Folder" });
    await newFolder.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("dialog")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toBeHidden();
    await expect(newFolder).toBeFocused();
  });

  test("the entry menu closes with Escape and focus returns to its button", async ({ app }) => {
    await openFileBrowser(app);
    const page = app.page;
    const trigger = rowMenu(page, "notes.txt");
    await trigger.click();
    await expect(page.getByRole("button", { name: "Properties", exact: true })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("button", { name: "Properties", exact: true })).toBeHidden();
    await expect(trigger).toBeFocused();
  });

  test("long names are truncated inside the panel and carry a tooltip", async ({ app }) => {
    await openFileBrowser(app);
    const page = app.page;
    const name = page.getByText(LONG_NAME, { exact: false }).first();
    await expect(name).toHaveAttribute("title", LONG_NAME);
    const clipped = await name.evaluate((el) => el.scrollWidth > el.clientWidth);
    expect(clipped, "long name is cut with an ellipsis").toBe(true);
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
    );
    expect(overflow, "page must not scroll horizontally").toBe(false);
  });

  test("screenshots for the review", async ({ app }) => {
    await openFileBrowser(app);
    const page = app.page;
    await captureScreen(page, "file-browser-list");
    await rowMenu(page, "notes.txt").click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await captureScreen(page, "file-browser-delete");
  });
});
