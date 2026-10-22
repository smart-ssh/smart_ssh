// Issue #578, Spec 0113: smoke check of the narrow-layout foundation. Tagged
// `@phone`: runs in the phone projects only (see playwright.config.ts).
import { expect, test } from "@playwright/test";

test.describe("narrow layout foundation @phone", () => {
  test("CSS variant and hook agree at 639 px and 640 px", async ({ page }) => {
    await page.goto("/e2e/harness/narrow.html");
    for (const [width, narrow] of [
      [639, true],
      [640, false],
      [390, true],
    ] as const) {
      await page.setViewportSize({ width, height: 844 });
      await expect(page.getByTestId("hook")).toHaveText(narrow ? "narrow" : "wide");
      if (narrow) await expect(page.getByTestId("css")).toBeVisible();
      else await expect(page.getByTestId("css")).toBeHidden();
    }
  });

  test("the app has no horizontal page scroll at phone width", async ({ page }) => {
    await page.goto("/e2e/harness/narrow.html");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });
});
