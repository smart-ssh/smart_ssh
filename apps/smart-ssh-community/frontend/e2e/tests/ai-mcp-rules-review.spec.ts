// Issue #386: UI review of the AI provider settings, the MCP settings and the
// filter rule management. Tab order with visible focus, Escape and focus
// return, and long content at both window sizes (docs/ui-guidelines.md).
import { expect, openSettings, test } from "../support/app";
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
  // Issue #1375: the tester is a side panel on wide windows, so the two
  // columns are each checked on their own (the list first, then the tester).
  await expectTabOrderFollowsVisualOrder(page, page.getByTestId("filter-rules-list"));
  await expectTabOrderFollowsVisualOrder(page, page.getByTestId("filter-rules-tester"));
  await expectNoClippedText(view);
  await captureScreen(page, "filter-rules");

  await page.getByRole("button", { name: "Edit" }).first().click();
  const ruleDialog = page.getByRole("dialog", { name: "Edit rule" });
  await expect(ruleDialog).toBeVisible();
  await expectTabOrderFollowsVisualOrder(page, ruleDialog);
  await expectNoClippedText(ruleDialog);
  await captureScreen(page, "filter-rules-edit");
});

// Issue #1377: both risk switches have accessible names, sit in the Tab order
// after the provider list and toggle with the keyboard.
test("settings: AI Provider risk switches are labelled, in Tab order and toggle", async ({ app }) => {
  const base = longContent();
  // Known non-default initial values: second opinion on (default off), red-risk confirm off (default on).
  await app.launch({
    ...base,
    settings: { ...base.settings, riskClassifierEnabled: true, redRiskAlwaysConfirm: false },
    commands: {
      ...base.commands,
      set_risk_second_opinion_settings: { result: null },
      set_red_risk_always_confirm: { result: null },
    },
  });
  const page = app.page;
  const dialog = await openSettings(page, "AI Provider");
  const second = dialog.getByRole("checkbox", { name: "Enable AI second opinion" });
  const red = dialog.getByRole("checkbox", { name: "Always confirm on red risk" });
  // Wait for the loaded (non-default) state before interacting.
  await expect(second).toBeChecked();
  await expect(red).not.toBeChecked();
  await second.focus();
  await expect(second).toBeFocused();
  // Tab order: second-opinion switch, its provider select (shown while on), red-risk switch.
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("combobox", { name: /provider/i }).first()).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(red).toBeFocused();
  await page.keyboard.press("Space");
  await expect(red).toBeChecked();
  // The switch keeps focus while the save runs and afterwards (no `disabled`).
  await expect(red).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(second).toBeFocused();
  await page.keyboard.press("Space");
  await expect(second).not.toBeChecked();
});

// Issue #1526: the red-risk switch keeps keyboard focus during a slow save,
// ignores a second toggle meanwhile and reverts (keeping focus) on failure.
test("settings: red-risk switch keeps focus while saving, ignores repeats, reverts on failure", async ({ app }) => {
  const base = longContent();
  await app.launch({
    ...base,
    settings: { ...base.settings, redRiskAlwaysConfirm: false },
    commands: { ...base.commands, set_red_risk_always_confirm: { hold: true } },
  });
  const page = app.page;
  const dialog = await openSettings(page, "AI Provider");
  const red = dialog.getByRole("checkbox", { name: "Always confirm on red risk" });
  await expect(red).not.toBeChecked();
  await red.focus();
  await page.keyboard.press("Space");
  await expect(red).toBeChecked();
  await expect(red).toHaveAttribute("aria-disabled", "true");
  await expect(red).toBeFocused();
  await page.keyboard.press("Space");
  await expect(red).toBeChecked();
  await expect(red).toBeFocused();
  await app.release("set_red_risk_always_confirm", { error: { code: "E2E", message: "save failed" } });
  await expect(red).not.toBeChecked();
  await expect(red).not.toHaveAttribute("aria-disabled", "true");
  await expect(red).toBeFocused();
  expect((await app.calls("set_red_risk_always_confirm")).length).toBe(1);
});
