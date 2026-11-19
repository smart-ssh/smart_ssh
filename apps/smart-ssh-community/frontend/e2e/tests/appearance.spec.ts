// Issue #804: appearance setting (System / Light / Dark).
import { readFileSync } from "node:fs";
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { captureScreen } from "../support/screenshots";

const en = JSON.parse(readFileSync(new URL("../../src/locales/en/common.json", import.meta.url), "utf8"));

async function openDisplaySettings(page: import("@playwright/test").Page) {
  await page.getByRole("button", { name: en.settings.title, exact: true }).click();
  await page.getByRole("button", { name: en.settings.categories.displayLanguage, exact: true }).click();
}

test.describe("appearance", () => {
  test.use({ colorScheme: "dark" });

  test("System follows the OS live, Light/Dark apply at once", async ({ app }) => {
    const { page } = app;
    await app.launch(populated());
    const html = page.locator("html");
    await expect(html).toHaveAttribute("data-theme", "dark");
    await page.emulateMedia({ colorScheme: "light" });
    await expect(html).toHaveAttribute("data-theme", "light");
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(html).toHaveAttribute("data-theme", "dark");

    await openDisplaySettings(page);
    await expect(page.getByRole("button", { name: en.appearance.system, exact: true })).toHaveAttribute("aria-pressed", "true");
    await page.getByRole("button", { name: en.appearance.light, exact: true }).click();
    await expect(html).toHaveAttribute("data-theme", "light");
    await expect(page.getByRole("button", { name: en.appearance.light, exact: true })).toHaveAttribute("aria-pressed", "true");
    // Explicit choice ignores the OS.
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(html).toHaveAttribute("data-theme", "light");
    await page.getByRole("button", { name: en.appearance.dark, exact: true }).click();
    await expect(html).toHaveAttribute("data-theme", "dark");
  });

  test("Angular tokens, square corners and the 2 px focus ring (issue #1362)", async ({ app }) => {
    const { page } = app;
    await app.launch(populated());
    const tokens = () =>
      page.evaluate(() => {
        const cs = getComputedStyle(document.documentElement);
        return ["--surface", "--accent", "--fg", "--border"].map((n) => cs.getPropertyValue(n).trim().toLowerCase());
      });
    const colours = async (v: string) =>
      page.evaluate((expr) => {
        const el = document.createElement("div");
        el.style.color = expr;
        document.body.appendChild(el);
        const c = getComputedStyle(el).color;
        el.remove();
        return c;
      }, v);
    const rgb = (hex: string) => {
      const n = parseInt(hex.slice(1), 16);
      return `rgb(${n >> 16}, ${(n >> 8) & 255}, ${n & 255})`;
    };
    const check = async (expected: string[]) => {
      const got = await tokens();
      for (let i = 0; i < expected.length; i++) expect(await colours(got[i])).toBe(rgb(expected[i]));
    };
    await check(["#0e1116", "#2cc6e8", "#e3e7ed", "#242b35"]);

    // Radius 0 on the main screen and in settings (spinner/pings are square, too).
    const roundedCount = () =>
      page.evaluate(
        () =>
          Array.from(document.querySelectorAll("*")).filter((el) => {
            const r = getComputedStyle(el).borderTopLeftRadius;
            return r !== "0px" && r !== "";
          }).map((el) => el.outerHTML.slice(0, 160)),
      );
    await expect.poll(roundedCount).toEqual([]);
    await page.getByRole("button", { name: en.settings.title, exact: true }).click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await expect.poll(roundedCount).toEqual([]);

    // Focus ring: 2 px, no offset, on a button.
    const button = page.locator(":focus-visible");
    await page.keyboard.press("Tab");
    await expect(page.locator(":focus-visible")).toHaveCount(1);
    const shadow = await button.evaluate((el) => getComputedStyle(el).boxShadow);
    expect(shadow).toContain("0px 0px 0px 2px");
  });

  test("main view in light mode", async ({ app }) => {
    const { page } = app;
    await app.launch(populated());
    await openDisplaySettings(page);
    await page.getByRole("button", { name: en.appearance.light, exact: true }).click();
    await page.getByRole("button", { name: en.common.close, exact: true }).click();
    await expect(page.getByText("db-primary")).toBeVisible();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await captureScreen(page, "appearance-light-main");
  });
});

test("the cached choice is applied before the app renders (no flash)", async ({ app }) => {
  await app.page.addInitScript(() => window.localStorage.setItem("smart-ssh.theme", "light"));
  // The pre-paint script runs from <head>; observe the attribute at DOMContentLoaded.
  await app.page.addInitScript(() => {
    document.addEventListener("DOMContentLoaded", () => {
      (window as unknown as { __themeAtDcl: string | undefined }).__themeAtDcl =
        document.documentElement.dataset.theme;
    });
  });
  await app.launch(populated());
  expect(await app.page.evaluate(() => (window as unknown as { __themeAtDcl?: string }).__themeAtDcl)).toBe("light");
});
