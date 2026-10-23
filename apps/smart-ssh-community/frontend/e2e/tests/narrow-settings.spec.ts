// Issue #586, Spec 0113 §5d: Settings and the filter rules view at phone width.
// Tagged `@phone`: runs in the phone projects only (390×844).
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { aiProvider, populated, server } from "../support/data";
import { expectFullyInViewport, expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";

const LONG = "a-very-long-name-without-any-break-".repeat(4);

const RULES = [
  { id: "r1", patternType: "glob", patternValue: "ls *", action: "Allow", scope: "Global", priority: 10, patternError: null },
  { id: "r2", patternType: "regex", patternValue: `rm -rf ${LONG}`, action: "Deny", scope: "Global", priority: 20, patternError: null },
  { id: "r3", patternType: "exact", patternValue: "reboot", action: "Confirm", scope: { Server: "s-db" }, priority: 5, patternError: null },
];

const content = () => ({
  ...populated(),
  aiProviders: [
    aiProvider({ id: "p1", displayName: LONG, model: LONG }),
    aiProvider({ id: "p2", displayName: "Second", isActive: false, providerType: "generic_openai_compatible", baseUrl: "http://x" }),
  ],
  servers: [server({ id: "s-db", name: LONG }), server({ id: "s-2", name: "web-1" })],
  commands: {
    list_rules: { result: RULES },
    set_active_ai_provider: { result: null },
    create_rule: { result: RULES[0] },
    update_rule: { result: RULES[0] },
    delete_rule: { result: null },
    evaluate_explained: {
      result: {
        decision: { Deny: { reason: `matched ${LONG}`, code: "RULE_DENY" } },
        matchedRule: "r2",
        matchedHardBlacklistEntry: null,
        subCommandTraces: [],
        kind: "Segment",
        segmentIndex: 0,
        checks: [],
      },
    },
    list_hard_blacklist: { result: [{ kind: "regex", value: LONG }] },
    get_mcp_server_settings: {
      result: { enabled: true, endpoint: `http://127.0.0.1:7777/${LONG}`, token: LONG, allowedServerIds: ["s-2"], confirmTimeoutSecs: 120 },
    },
  },
});

async function noHorizontalScroll(page: Page, scope?: Locator) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  if (scope) {
    // The scrolling content box itself must not scroll sideways (its own overflow would hide a wide child).
    for (const box of await scope.locator("div.overflow-y-auto").all()) {
      expect(await box.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
    }
    // Nothing inside the dialog is wider than the dialog, except boxes that scroll on their own.
    const overflow = await scope.evaluate((root) => {
      const limit = root.getBoundingClientRect().right + 1;
      const bad: string[] = [];
      for (const el of root.querySelectorAll<HTMLElement>("*")) {
        const r = el.getBoundingClientRect();
        if (r.width === 0 || r.right <= limit) continue;
        let p: HTMLElement | null = el.parentElement;
        let scrolls = false;
        while (p && p !== root) {
          const ox = getComputedStyle(p).overflowX;
          if (ox === "auto" || ox === "scroll" || ox === "hidden") scrolls = true;
          p = p.parentElement;
        }
        if (!scrolls) bad.push(`${el.tagName}.${el.className}`.slice(0, 80));
      }
      return bad;
    });
    expect(overflow).toEqual([]);
  }
}

async function openSettings(page: Page) {
  await page.getByRole("button", { name: "Open navigation menu" }).click();
  await page.getByRole("dialog", { name: "Navigation menu" }).getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  return dialog;
}

test.describe("narrow settings @phone", () => {
  test("full-screen section list; every section opens, fits and returns via Back", async ({ app }, info) => {
    test.setTimeout(120_000);
    await app.launch(content());
    const page = app.page;
    await page.evaluate(() => {
      (window as unknown as { __e2eExtensions: { registerTestSettingsSection: (id: string, label: string) => void } }).__e2eExtensions.registerTestSettingsSection("ext-test", "Extension section");
    });
    const dialog = await openSettings(page);
    const box = await dialog.boundingBox();
    expect(box!.width).toBeGreaterThanOrEqual(page.viewportSize()!.width - 1);
    await expectFullyInViewport(dialog.getByRole("button", { name: "Close", exact: true }));
    const list = dialog.locator("nav ul button");
    const count = await list.count();
    expect(count).toBeGreaterThan(5);
    await expect(dialog.getByRole("button", { name: "Extension section", exact: true })).toBeVisible();
    await noHorizontalScroll(page, dialog);
    await expectNoSeriousA11yViolations(page, "settings: narrow list");
    await captureScreen(page, `narrow-settings-list-${info.project.name}`);

    for (let i = 0; i < count; i++) {
      const item = dialog.locator("nav ul button").nth(i);
      const label = (await item.innerText()).trim();
      await item.click();
      const back = dialog.getByRole("button", { name: /Sections/ });
      await expectUsable(back);
      await expectFullyInViewport(dialog.getByRole("button", { name: "Close", exact: true }));
      await expect(dialog.getByRole("heading", { level: 3, name: label })).toBeVisible();
      await noHorizontalScroll(page, dialog);
      await expectNoSeriousA11yViolations(page, `settings: ${label}`);
      if (label === "Extension section") await expect(dialog.getByTestId("ext-section")).toBeVisible();
      if (i < 2) await captureScreen(page, `narrow-settings-${i}-${info.project.name}`);
      await back.click();
      await expect(dialog.locator("nav ul button").nth(i)).toBeFocused();
    }
  });

  test("AI provider settings: add and activate a provider", async ({ app }, info) => {
    test.setTimeout(90_000);
    await app.launch(content());
    const page = app.page;
    const dialog = await openSettings(page);
    await dialog.getByRole("button", { name: "AI Provider", exact: true }).click();
    await noHorizontalScroll(page, dialog);
    await captureScreen(page, `narrow-settings-ai-${info.project.name}`);
    await dialog.getByRole("button", { name: "Add Provider", exact: true }).click();
    const add = page.getByRole("dialog", { name: "Add Provider" });
    await expect(add).toBeVisible();
    await noHorizontalScroll(page, add);
    await add.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(add).toBeHidden();
    // The provider list's row actions are reachable: make the second provider active.
    const setActive = dialog.getByRole("button", { name: "Set Active", exact: true });
    await expectUsable(setActive);
    await expectUsable(dialog.getByRole("button", { name: "Delete", exact: true }).first());
    await setActive.click();
    await expect.poll(async () => (await app.calls("set_active_ai_provider")).length).toBe(1);
    await noHorizontalScroll(page, dialog);
  });

  test("filter rules: cards, create, edit, delete and the test panel", async ({ app }, info) => {
    test.setTimeout(90_000);
    await app.launch(content());
    const page = app.page;
    await page.getByRole("button", { name: "Open navigation menu" }).click();
    await page.getByRole("dialog", { name: "Navigation menu" }).getByRole("button", { name: "Filter Rules", exact: true }).click();
    const heading = page.getByRole("heading", { name: "Rules", level: 2 });
    await expect(heading).toBeVisible();
    const view = page.locator("div.overflow-y-auto", { has: heading }).last();
    await expectUsable(page.getByRole("button", { name: "+ Rule", exact: true }));
    for (const name of ["Edit", "Delete", "Increase priority", "Decrease priority"]) {
      for (const b of await page.getByRole("button", { name, exact: true }).all()) {
        if (await b.isEnabled()) await expectUsable(b);
      }
    }
    await noHorizontalScroll(page, view);
    await expectNoSeriousA11yViolations(page, "filter rules");
    await captureScreen(page, `narrow-rules-${info.project.name}`);

    // Edit
    await page.getByRole("button", { name: "Edit", exact: true }).nth(1).click();
    await expect(page.getByRole("heading", { name: "Edit Rule" })).toBeVisible();
    await noHorizontalScroll(page, view);
    await captureScreen(page, `narrow-rules-edit-${info.project.name}`);
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect.poll(async () => (await app.calls("update_rule")).length).toBe(1);

    // Create
    await page.getByRole("button", { name: "+ Rule", exact: true }).click();
    await expect(page.getByRole("heading", { name: "New Rule" })).toBeVisible();
    await page.getByLabel("Pattern", { exact: true }).fill("systemctl status *");
    await page.getByRole("radio", { name: "Tag" }).check();
    await page.getByRole("combobox", { name: "Tag" }).fill("production");
    await noHorizontalScroll(page, view);
    await expectUsable(page.getByRole("button", { name: "Create", exact: true }));
    await page.getByRole("button", { name: "Create", exact: true }).click();
    await expect.poll(async () => (await app.calls("create_rule")).length).toBe(1);

    // Delete
    await page.getByRole("button", { name: "Delete", exact: true }).first().click();
    await expect.poll(async () => (await app.calls("delete_rule")).length).toBe(1);

    // Explain / test a command
    await page.getByLabel("Example Command").fill(`rm -rf ${LONG}`);
    const test_ = page.getByRole("button", { name: "Test", exact: true });
    await test_.scrollIntoViewIfNeeded();
    await expectUsable(test_);
    await test_.click();
    await expect(page.getByText("Overall Decision")).toBeVisible();
    await noHorizontalScroll(page, view);
    await expectNoSeriousA11yViolations(page, "filter rules: test panel");
    await captureScreen(page, `narrow-rules-test-${info.project.name}`);
  });
});
