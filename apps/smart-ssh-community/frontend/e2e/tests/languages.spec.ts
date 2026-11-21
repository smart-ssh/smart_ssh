// Case 9: switching between German and English leaves no raw i18n keys on
// the main screens and in the settings.
import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { populated } from "../support/data";

type Catalog = Record<string, unknown>;
const load = (lang: string): Catalog =>
  JSON.parse(readFileSync(new URL(`../../src/locales/${lang}/common.json`, import.meta.url), "utf8")) as Catalog;
const de = load("de");
const en = load("en");
const CATALOGS: Record<"de" | "en", Catalog> = { de, en };
const SECTIONS = new Set(Object.keys(en));

function tr(catalog: Catalog, path: string): string {
  const value = path.split(".").reduce<unknown>((node, key) => (node as Record<string, unknown>)?.[key], catalog);
  if (typeof value !== "string") throw new Error(`missing translation ${path}`);
  return value;
}

/**
 * Strings on screen that look like an untranslated key: `section.key` or
 * deeper, camelCase segments, where `section` is a top-level section of the
 * catalog. Covers visible text and the visible labels of elements
 * (placeholder, title, aria-label).
 */
async function rawKeysOnScreen(page: Page): Promise<string[]> {
  const candidates = await page.evaluate(() => {
    const texts = [document.body.innerText];
    for (const el of document.querySelectorAll<HTMLElement>("[placeholder],[title],[aria-label]")) {
      if (el.offsetParent === null && getComputedStyle(el).position !== "fixed") continue;
      for (const attr of ["placeholder", "title", "aria-label"]) {
        const value = el.getAttribute(attr);
        if (value) texts.push(value);
      }
    }
    return texts.join("\n").split(/[\s"'()[\]{}:,;!?…«»„“”]+/);
  });
  const keyShape = /^([a-z][A-Za-z0-9]*)(\.[A-Za-z][A-Za-z0-9_]*)+\.?$/;
  return [...new Set(candidates.filter((token) => {
    const match = keyShape.exec(token);
    return match !== null && SECTIONS.has(match[1]);
  }))];
}

async function expectNoRawKeys(page: Page, where: string) {
  await expect.poll(() => rawKeysOnScreen(page), { message: `raw i18n keys on ${where}` }).toEqual([]);
}

// The picker labels each language by its own name, whatever the UI language.
const ENDONYMS = { de: "Deutsch", en: "English" } as const;

async function switchLanguage(page: Page, from: Catalog, to: "de" | "en") {
  await page.getByRole("button", { name: tr(from, "settings.title"), exact: true }).click();
  await page.getByRole("button", { name: tr(from, "settings.categories.displayLanguage"), exact: true }).click();
  await page.getByRole("button", { name: ENDONYMS[to], exact: true }).click();
  // The UI switches at once: the settings title is now in the new language.
  await expect(page.getByRole("heading", { name: tr(CATALOGS[to], "settings.title"), exact: true })).toBeVisible();
  await page.getByRole("button", { name: tr(CATALOGS[to], "common.close"), exact: true }).click();
}

async function checkMainScreens(page: Page, c: Catalog) {
  await page.getByRole("button", { name: tr(c, "nav.connect"), exact: true }).click();
  await expect(page.getByText("db-primary")).toBeVisible();
  await expectNoRawKeys(page, "Connect");

  await page.getByRole("button", { name: tr(c, "nav.manage"), exact: true }).click();
  await page.getByRole("treeitem", { name: "db-primary", exact: true }).click();
  for (const tab of ["general", "connection", "security"]) {
    await page.getByRole("tab", { name: tr(c, `serverForm.tabs.${tab}`), exact: true }).click();
    await expectNoRawKeys(page, `Manage (server form, ${tab})`);
  }
  await page.getByRole("tab", { name: tr(c, "common.notes"), exact: true }).click();
  await expectNoRawKeys(page, "Manage (server form, notes)");
  await page.getByRole("tab", { name: tr(c, "serverForm.tabs.connection"), exact: true }).click();
  await expect(page.getByRole("button", { name: tr(c, "serverForm.testConnection"), exact: true })).toBeVisible();

  await page.getByRole("button", { name: tr(c, "nav.rules"), exact: true }).click();
  await expectNoRawKeys(page, "Filter rules");
}

async function checkSettings(page: Page, c: Catalog) {
  await page.getByRole("button", { name: tr(c, "settings.title"), exact: true }).click();
  const categories = page.locator("nav ul button");
  const count = await categories.count();
  expect(count).toBeGreaterThan(3);
  for (let i = 0; i < count; i++) {
    const category = categories.nth(i);
    await category.click();
    await expect(category).toHaveAttribute("aria-current", "page");
    await expectNoRawKeys(page, `Settings → ${await category.innerText()}`);
  }
  await page.getByRole("button", { name: tr(c, "common.close"), exact: true }).click();
}

test("the selected language segment has a 2 px accent bottom border, the other has none", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await page.getByRole("button", { name: tr(en, "settings.title"), exact: true }).click();
  await page.getByRole("button", { name: tr(en, "settings.categories.displayLanguage"), exact: true }).click();
  const accent = await page.evaluate(() => {
    const probe = document.createElement("div");
    probe.className = "bg-accent";
    document.body.appendChild(probe);
    const colour = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return colour;
  });
  const deBtn = page.getByRole("button", { name: ENDONYMS.de, exact: true });
  const enBtn = page.getByRole("button", { name: ENDONYMS.en, exact: true });
  await expect(enBtn).toHaveCSS("border-bottom-color", accent);
  await expect(enBtn).toHaveCSS("border-bottom-width", "2px");
  await expect(deBtn).not.toHaveCSS("border-bottom-color", accent);
  await deBtn.click();
  await expect(deBtn).toHaveAttribute("aria-pressed", "true");
  await expect(deBtn).toHaveCSS("border-bottom-color", accent);
  await expect(deBtn).toHaveCSS("border-bottom-width", "2px");
  await expect(enBtn).not.toHaveCSS("border-bottom-color", accent);
});

test("German and English show no raw i18n keys on the main screens and in the settings", async ({ app }) => {
  test.setTimeout(90_000);
  await app.launch(populated());
  const page = app.page;

  await switchLanguage(page, en, "de");
  await checkMainScreens(page, de);
  await checkSettings(page, de);

  await switchLanguage(page, de, "en");
  await checkMainScreens(page, en);
  await checkSettings(page, en);
});
