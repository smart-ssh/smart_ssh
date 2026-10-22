// Issue #386: UI review of the AI provider settings, the MCP settings and the
// filter rule management. Tab order with visible focus, Escape and focus
// return, and long content at both window sizes (docs/ui-guidelines.md).
import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { aiProvider, populated, server } from "../support/data";
import { expectNoClippedText, expectTabOrderFollowsVisualOrder, expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";

const LONG = "a-very-long-name-without-any-break-".repeat(5);

const RULES = [
  { id: "r1", patternType: "glob", patternValue: "ls *", action: "Allow", scope: "Global", priority: 10, patternError: null },
  {
    id: "r2",
    patternType: "regex",
    patternValue: `rm -rf ${LONG}`,
    action: "Deny",
    scope: "Global",
    priority: 20,
    patternError: null,
  },
  { id: "r3", patternType: "exact", patternValue: "reboot", action: "Confirm", scope: { Server: "s-db" }, priority: 5, patternError: null },
];

const longContent = () => ({
  ...populated(),
  aiProviders: [
    aiProvider({ id: "p1", displayName: LONG, model: LONG }),
    aiProvider({ id: "p2", displayName: "Second", isActive: false, providerType: "generic_openai_compatible", baseUrl: "http://x" }),
  ],
  servers: [server({ id: "s-db", name: LONG }), server({ id: "s-2", name: "web-1" })],
  commands: {
    list_rules: { result: RULES },
    list_hard_blacklist: { result: [{ kind: "regex", value: LONG }] },
    get_mcp_server_settings: {
      result: {
        enabled: true,
        endpoint: "http://127.0.0.1:7777/" + LONG,
        token: LONG,
        allowedServerIds: ["s-2"],
        confirmTimeoutSecs: 120,
      },
    },
  },
});

async function openSettings(page: Page, category: string) {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: category, exact: true }).click();
  return dialog;
}

for (const category of ["AI Provider", "MCP Server"]) {
  test(`settings: ${category} has a Tab order that follows the visual order, with visible focus`, async ({ app }) => {
    test.setTimeout(90_000);
    await app.launch(longContent());
    const page = app.page;
    const dialog = await openSettings(page, category);
    // Clicking inside the section scrolled it; measure the order from the top.
    await dialog.locator("nav ~ div").evaluate((root) => {
      for (const el of [root, ...Array.from(root.querySelectorAll<HTMLElement>("*"))]) el.scrollTop = 0;
    });
    await expectTabOrderFollowsVisualOrder(page, dialog.locator("nav ~ div"));
    await expectNoClippedText(dialog.locator("nav ~ div"));
    await expectUsable(dialog.getByRole("button", { name: "Close", exact: true }));
    await captureScreen(page, `settings-${category.replace(/\W/g, "").toLowerCase()}`);
  });

  if (category === "AI Provider") {
    // Issue #603: the add form moved into its own dialog.
    test("settings: the add-provider dialog has a Tab order that follows the visual order, with visible focus", async ({ app }) => {
      test.setTimeout(90_000);
      await app.launch(longContent());
      const page = app.page;
      const settings = await openSettings(page, category);
      await settings.getByRole("button", { name: "Add Provider", exact: true }).click();
      const add = page.getByRole("dialog", { name: "Add Provider" });
      await add.getByRole("button", { name: /Advanced/ }).click();
      await add.getByRole("button", { name: "+ Header", exact: true }).click();
      await expectTabOrderFollowsVisualOrder(page, add);
      await expectNoClippedText(add);
    });
  }

  test(`settings: Escape closes the dialog from ${category} and focus returns to the trigger`, async ({ app }) => {
    await app.launch(longContent());
    const page = app.page;
    const trigger = page.getByRole("button", { name: "Settings", exact: true });
    await trigger.focus();
    const dialog = await openSettings(page, category);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
  });
}

test("filter rules: Tab order, edit form and test panel, long values", async ({ app }) => {
  test.setTimeout(90_000);
  await app.launch(longContent());
  const page = app.page;
  await page.getByRole("button", { name: "Filter Rules", exact: true }).click();
  const heading = page.getByRole("heading", { name: "Rules", level: 2 });
  await expect(heading).toBeVisible();
  const view = page.locator("div.overflow-y-auto", { has: heading }).last();
  await expectTabOrderFollowsVisualOrder(page, view);
  await expectNoClippedText(view);
  await captureScreen(page, "filter-rules");

  await page.getByRole("button", { name: "Edit" }).first().click();
  await expect(page.getByRole("heading", { name: "Edit Rule" })).toBeVisible();
  await expectTabOrderFollowsVisualOrder(page, view);
  await expectNoClippedText(view);
  await captureScreen(page, "filter-rules-edit");
});
