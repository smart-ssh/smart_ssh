// Issue #385: UI review of the settings and the notes. Keyboard order and
// focus indicators, Escape and focus return of the dialogs, and long content
// at both window sizes (docs/ui-guidelines.md).
import type { Page } from "@playwright/test";
import { THIRD_PARTY_NOTICES_MARKER } from "../../src/thirdPartyNoticesMarker";
import { expect, test } from "../support/app";
import { populated, server } from "../support/data";
import { expectNoClippedText, expectTabOrderFollowsVisualOrder, expectUsable } from "../support/layout";
import { NOTE_COMMANDS, openNotes } from "../support/notes";

const LONG_PATH = `/home/someone-with-a-long-name/.local/share/smart-ssh/${"nested-directory/".repeat(8)}smart-ssh.db`;

const longContent = () => ({
  ...populated(),
  commands: {
    get_data_paths: {
      result: [
        { id: "database", label: null, path: LONG_PATH, isDirectory: false },
        { id: "logs", label: null, path: LONG_PATH.replace(".db", "-logs"), isDirectory: true },
      ],
    },
    get_app_info: {
      result: {
        version: "0.6.0",
        versionDisplay: "0.6.0-rc.1+build.1234567890abcdef1234567890abcdef12345678",
        edition: "Community Edition with a very long edition name",
        buildType: "Dev",
      },
    },
    get_master_password_mode: { result: "password" },
  },
});

async function openSettings(page: Page, category?: string) {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  if (category) await dialog.getByRole("button", { name: category, exact: true }).click();
  return dialog;
}

test("settings: focus starts on the active category, Escape closes and focus returns to the trigger", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  const trigger = page.getByRole("button", { name: "Settings", exact: true });
  await trigger.focus();
  const dialog = await openSettings(page);
  await expect(dialog.locator("nav button[aria-current=page]")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(trigger).toBeFocused();
});

test("settings: Tab stays inside the dialog", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  const dialog = await openSettings(page, "Display & Language");
  const inside = () => page.evaluate(() => !!document.activeElement?.closest('[role="dialog"]'));
  for (let i = 0; i < 25; i++) {
    await page.keyboard.press("Tab");
    expect(await inside()).toBe(true);
  }
  for (let i = 0; i < 25; i++) {
    await page.keyboard.press("Shift+Tab");
    expect(await inside()).toBe(true);
  }
  await expect(dialog).toBeVisible();
});

test("settings: the licenses dialog closes with Escape alone and returns focus to its button", async ({ app }) => {
  await page404(app);
  await app.launch(populated());
  const page = app.page;
  const settings = await openSettings(page, "About");
  const open = settings.getByRole("button", { name: "Third-party licenses", exact: true });
  await open.click();
  const licenses = page.getByRole("dialog", { name: "Third-party licenses" });
  await expect(licenses).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(licenses).toBeHidden();
  await expect(settings).toBeVisible();
  await expect(open).toBeFocused();
});

async function page404(app: { page: Page }) {
  await app.page.route("**/third-party-notices.txt", (route) =>
    route.fulfill({ status: 200, contentType: "text/plain", body: `${THIRD_PARTY_NOTICES_MARKER}\npackage — MIT` }),
  );
}

test("settings: the overwrite question closes with Escape and focus stays in the form", async ({ app }) => {
  await app.launch({
    ...populated(),
    commands: {
      get_master_password_mode: { result: "password" },
      switch_to_os_keychain: { error: { code: "KEYCHAIN_HOLDS_ANOTHER_KEY", message: "another key" } },
    },
  });
  const page = app.page;
  const settings = await openSettings(page, "Master password");
  await settings.getByLabel("Master password, to confirm").fill("a-master-password");
  const switchButton = settings.getByRole("button", { name: "Switch to the keychain", exact: true });
  await switchButton.click();
  const question = page.getByRole("dialog");
  await expect(question.filter({ hasText: "Cancel" }).last()).toBeVisible();
  const cancel = page.getByRole("button", { name: "Cancel", exact: true });
  await expect(cancel).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(cancel).toBeHidden();
  await expect(settings).toBeVisible();
  // The typed password is cleared, so the button the question came from is
  // disabled; focus lands on the (empty) password field instead of getting lost.
  await expect(settings.getByLabel("Master password, to confirm")).toBeFocused();
});

test("settings: every category has a Tab order that follows the visual order, with visible focus", async ({ app }) => {
  test.setTimeout(90_000);
  await app.launch(longContent());
  const page = app.page;
  const dialog = await openSettings(page);
  await expectTabOrderFollowsVisualOrder(page, dialog.locator("nav"));
  const categories = dialog.locator("nav ul button");
  const count = await categories.count();
  for (let i = 0; i < count; i++) {
    await categories.nth(i).click();
    await expectTabOrderFollowsVisualOrder(page, dialog.locator("nav ~ div"));
  }
});

test("settings: long values neither stick out nor get cut off, in every category", async ({ app }) => {
  test.setTimeout(90_000);
  await app.launch(longContent());
  const page = app.page;
  const dialog = await openSettings(page);
  const categories = dialog.locator("nav ul button");
  const count = await categories.count();
  for (let i = 0; i < count; i++) {
    await categories.nth(i).click();
    await expectNoClippedText(dialog.locator("nav ~ div"));
  }
  await expectUsable(dialog.getByRole("button", { name: "Close", exact: true }));
});

test("notes: Tab order, long note and history at both sizes", async ({ app }) => {
  const notes = "x".repeat(4500);
  await app.launch({
    ...populated(),
    servers: [server({ id: "db-primary", name: "db-primary", notes })],
    commands: NOTE_COMMANDS,
  });
  const page = app.page;
  await openNotes(app);
  await expect(page.getByText("This note is very large", { exact: false })).toBeVisible();
  await page.getByRole("button", { name: "Show history", exact: true }).click();
  await page.getByRole("button", { name: /User/ }).first().click();
  const panel = page.locator("div.space-y-2", { has: page.getByRole("button", { name: "Hide history" }) }).last();
  await expectTabOrderFollowsVisualOrder(page, panel);
  await expectNoClippedText(panel);
});
