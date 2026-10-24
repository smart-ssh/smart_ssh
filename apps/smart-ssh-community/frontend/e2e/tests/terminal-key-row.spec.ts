import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { openSession } from "../support/session";

// Issue #678, Spec 0114: touch key row in the terminal tab.
test.describe("terminal key row @phone", () => {
  test.use({ hasTouch: true, isMobile: true });

  test("is visible with a coarse pointer and has 44px targets", async ({ app }) => {
    await app.launch(populated());
    await openSession(app, "web-1");
    const page = app.page;
    await page.getByRole("tab", { name: "Terminal", exact: true }).click();
    const row = page.getByTestId("terminal-key-row").locator("visible=true");
    await expect(row).toBeVisible();
    for (const name of ["Esc", "Ctrl", "Alt", "Tab"]) {
      const box = await row.getByRole("button", { name, exact: true }).boundingBox();
      expect(box?.height).toBeGreaterThanOrEqual(44);
      expect(box?.width).toBeGreaterThanOrEqual(44);
    }
  });
});

test.describe("terminal key row on desktop", () => {
  test("is absent without a touch pointer", async ({ app }) => {
    await app.launch(populated());
    await openSession(app, "web-1");
    await expect(app.page.locator(".xterm-screen").locator("visible=true")).toBeVisible();
    await expect(app.page.getByTestId("terminal-key-row")).toHaveCount(0);
  });
});
