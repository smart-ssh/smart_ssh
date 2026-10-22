// Issue #579, Spec 0113 §3: burger menu of the narrow app shell. Tagged
// `@phone`: runs in the phone projects only (390×844).
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { expectFullyInViewport, expectTopmostAtCenter } from "../support/layout";
import { captureScreen } from "../support/screenshots";

test.describe("narrow shell @phone", () => {
  test("title bar shows burger and title; the menu lists navigation and sessions", async ({ app }, info) => {
    await app.launch(populated());
    const page = app.page;
    const burger = page.getByRole("button", { name: "Open navigation menu" });
    await expect(burger).toBeVisible();
    await expectFullyInViewport(burger);
    await expect(page.getByTestId("narrow-title")).toHaveText("Smart SSH");
    await expect(page.getByRole("navigation")).toHaveCount(0);
    await expect(page.getByRole("toolbar")).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await captureScreen(page, `narrow-shell-overview-${info.project.name}`);

    // The session view itself is laid out separately; here only the shell matters.
    await page.getByRole("button", { name: /^db-primary\b/ }).click();
    await expect.poll(async () => (await app.calls("connect")).length).toBe(1);
    await expect(page.getByTestId("narrow-title")).toContainText("db-primary");
    await burger.click();
    const menu = page.getByRole("dialog", { name: "Navigation menu" });
    await expect(menu).toBeVisible();
    for (const name of ["Overview", "Connect", "Manage", "Filter Rules", "Settings", "db-primary", 'Close tab "db-primary"']) {
      const entry = menu.getByRole("button", { name, exact: true });
      await expectFullyInViewport(entry);
      await expectTopmostAtCenter(entry);
    }
    await expect(menu.getByRole("button", { name: "db-primary", exact: true })).toHaveAttribute("aria-current", "page");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await expectNoSeriousA11yViolations(page, "narrow menu");
    await captureScreen(page, `narrow-shell-menu-${info.project.name}`);
  });

  test("selecting closes the menu and returns focus; Escape and a tap outside close it", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    const burger = page.getByRole("button", { name: "Open navigation menu" });
    const menu = page.getByRole("dialog", { name: "Navigation menu" });

    await burger.click();
    await menu.getByRole("button", { name: "Manage", exact: true }).click();
    await expect(menu).toBeHidden();
    await expect(burger).toBeFocused();

    await burger.click();
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
    await expect(burger).toBeFocused();

    await burger.click();
    await page.mouse.click(385, 500);
    await expect(menu).toBeHidden();
    await expect(burger).toBeFocused();

    await burger.click();
    await page.setViewportSize({ width: 800, height: 560 });
    await expect(menu).toBeHidden();
    await expect(burger).toBeHidden();
    await expect(page.getByRole("button", { name: "Connect", exact: true })).toBeVisible();
  });
});
