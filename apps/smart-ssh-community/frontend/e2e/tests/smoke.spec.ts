// Case 1: the app starts against an empty and a populated backend; the
// main layout (navigation, server list, sidebar, session area) renders in
// both window sizes without console errors.
import type { Page } from "@playwright/test";
import { expect, test, type AppDriver } from "../support/app";
import { EMPTY, populated } from "../support/data";
import { expectUsable } from "../support/layout";
import { chatInput, openSession } from "../support/session";

async function expectNavigationUsable(page: Page) {
  for (const name of ["Connect", "Manage", "Filter Rules", "Settings"]) {
    await expectUsable(page.getByRole("button", { name, exact: true }));
  }
}

/** The "Manage" sidebar with its actions and the given tree entries. */
async function expectSidebar(app: AppDriver, entries: string[]) {
  const page = app.page;
  await page.getByRole("button", { name: "Manage", exact: true }).click();
  await expectUsable(page.getByRole("button", { name: "+ Server", exact: true }));
  await expectUsable(page.getByRole("button", { name: "+ Group", exact: true }));
  for (const entry of entries) {
    await expect(page.getByRole("button", { name: entry, exact: true })).toBeVisible();
  }
  if (entries.length === 0) await expect(page.getByText("Nothing created yet.")).toBeVisible();
  await page.getByRole("button", { name: "Connect", exact: true }).click();
}

test.describe("smoke", () => {
  test("starts with an empty backend", async ({ app }) => {
    await app.launch(EMPTY);
    const page = app.page;
    await expect(page.getByRole("heading", { name: "Smart SSH" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Servers" })).toBeVisible();
    await expectNavigationUsable(page);
    await expectSidebar(app, []);
    expect(app.consoleErrors).toEqual([]);
  });

  test("starts with a populated backend and opens a session", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    await expect(page.getByRole("heading", { name: "Servers" })).toBeVisible();
    for (const name of ["db-primary", "web-1", "lab-box", "standalone"]) {
      await expect(page.getByRole("button", { name: new RegExp(`^${name}\\b`) })).toBeVisible();
    }
    await expectNavigationUsable(page);
    await expectSidebar(app, ["📁 Production", "📁 Web", "📁 Lab", "🖥️ standalone"]);

    // Session area: chat with its input, and the terminal.
    await openSession(app, "db-primary");
    await expectUsable(chatInput(page));
    await expect(page.getByRole("textbox", { name: "Terminal input" })).toBeAttached();
    await expectUsable(page.getByRole("button", { name: "Overview", exact: true }));
    expect(app.consoleErrors).toEqual([]);
  });
});
