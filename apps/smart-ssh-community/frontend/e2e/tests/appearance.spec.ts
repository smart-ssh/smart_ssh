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

test.describe("light theme focus and letter spacing (issue #1397)", () => {
  type Ring = { shadow: string; outline: string; outlineWidth: string; outlineStyle: string; bg: string; focus: string };
  const ringOf = (page: import("@playwright/test").Page) =>
    page.evaluate((): Ring => {
      const el = document.activeElement as HTMLElement;
      const cs = getComputedStyle(el);
      const probe = document.createElement("div");
      probe.style.color = getComputedStyle(document.documentElement).getPropertyValue("--focus");
      document.body.appendChild(probe);
      const focus = getComputedStyle(probe).color;
      probe.remove();
      return {
        shadow: cs.boxShadow,
        outline: cs.outlineColor,
        outlineWidth: cs.outlineWidth,
        outlineStyle: cs.outlineStyle,
        bg: cs.backgroundColor,
        focus,
      };
    });
  const expectBoxRing = async (page: import("@playwright/test").Page) => {
    const r = await ringOf(page);
    expect(r.shadow).toContain("0px 0px 0px 2px");
    expect(r.shadow).toContain(r.focus);
    expect(r.focus).not.toBe(r.bg);
  };

  test.beforeEach(async ({ app }) => {
    await app.page.addInitScript(() => window.localStorage.setItem("smart-ssh.theme", "light"));
  });

  test("input shows a 2 px focus outline", async ({ app }) => {
    const { page } = app;
    await app.launch({ ...populated(), commands: { sftp_list: { result: [] } } });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");

    // Field: 2 px outline in the focus colour.
    await page.getByRole("button", { name: "Manage", exact: true }).click();
    await page.getByRole("button", { name: "+ Server", exact: true }).click();
    const input = page.getByRole("textbox").first();
    await expect(input).toBeVisible();
    await input.focus();
    await expect(input).toBeFocused();
    const f = await ringOf(page);
    expect(f.outlineStyle).toBe("solid");
    expect(f.outlineWidth).toBe("2px");
    expect(f.outline).toBe(f.focus);
    expect(f.outline).not.toBe(f.bg);
  });

  test("tab shows the 2 px ring", async ({ app }) => {
    const { page } = app;
    await app.launch(populated());
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.getByRole("button", { name: "Manage", exact: true }).click();
    await page.getByRole("treeitem", { name: /db-primary/ }).click();
    const general = page.getByRole("tab", { name: "General", exact: true });
    await expect(general).toHaveAttribute("aria-selected", "true");
    await general.focus();
    // Keyboard navigation makes the focus visible on the tab.
    await page.keyboard.press("ArrowRight");
    await expect(page.getByRole("tab", { name: "Connection", exact: true })).toBeFocused();
    await expectBoxRing(page);
  });

  test("menu item shows the 2 px ring", async ({ app }) => {
    const { page } = app;
    const entry = {
      name: "notes.txt", path: "/home/deploy/notes.txt", isDir: false, size: 2048, permissions: "rw-r--r--",
      modified: "2026-01-15T10:30:00Z", permissionsOctal: 0o644, uid: 1000, gid: 1000, owner: "deploy",
      group: "deploy", isSymlink: false, linkTarget: null, linkKind: null,
    };
    await app.launch({ ...populated(), commands: { sftp_list: { result: [entry] } } });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.getByRole("button", { name: /^db-primary\b/ }).click();
    await expect.poll(async () => (await app.calls("connect")).length).toBe(1);
    await page.getByRole("button", { name: "Files", exact: true }).click();
    await expect(page.getByText("notes.txt")).toBeVisible();
    await page.getByRole("button", { name: /^Actions: notes.txt$/ }).click();
    const items = page.getByRole("menuitem");
    await expect(items.first()).toBeFocused();
    await page.keyboard.press("ArrowDown");
    await expect(items.nth(1)).toBeFocused();
    await expectBoxRing(page);
  });

  test("lang=ja resets the label letter spacing, en keeps it", async ({ app }) => {
    const { page } = app;
    await app.launch(populated());
    await page.getByRole("button", { name: "Manage", exact: true }).click();
    await page.getByRole("button", { name: "+ Server", exact: true }).click();
    const label = page.locator("label.text-label").first();
    await expect(label).toBeVisible();
    await page.evaluate(() => (document.documentElement.lang = "en"));
    await expect.poll(() => label.evaluate((el) => getComputedStyle(el).letterSpacing)).not.toBe("normal");
    await page.evaluate(() => (document.documentElement.lang = "ja"));
    await expect.poll(() => label.evaluate((el) => getComputedStyle(el).letterSpacing)).toBe("normal");
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
