// Issue #381: UI review of the app shell (header, tabs, session view, toasts).
// Tab order and focus indicators, Escape and focus return, long content at both
// window sizes (docs/ui-guidelines.md). Screenshots for the review PR are
// written with SCREENSHOT_LABEL=before|after.
import type { Page } from "@playwright/test";
import { expect, test, type AppDriver } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { populated, server } from "../support/data";
import { expectNoClippedText, expectTabOrderFollowsVisualOrder, expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";
import { openSession } from "../support/session";

const LONG_NAME = `production-database-cluster-${"eu-central-".repeat(6)}primary`;

const longServers = () => ({
  ...populated(),
  servers: [
    server({ id: "db-primary", name: "db-primary" }),
    server({ id: "web-1", name: "web-1" }),
    server({ id: "long", name: LONG_NAME }),
    server({ id: "lab", name: "lab-box" }),
  ],
});

async function showToasts(page: Page) {
  await page.evaluate(async () => {
    const path = "/src/toastBus.ts";
    const bus = await import(/* @vite-ignore */ path);
    bus.showToast({ kind: "error", message: `Could not save the settings: ${"a very long reason ".repeat(8)}` });
    bus.showToast({ kind: "info", message: "Heads up" });
    bus.showToast({ kind: "info", message: "Saved", action: { label: "Undo", onClick: () => {} } });
  });
  await expect(page.getByText("Heads up")).toBeVisible();
}

async function openSeveral(app: AppDriver) {
  await openSession(app, "db-primary");
  await app.page.getByRole("button", { name: "Overview", exact: true }).click();
  await openSession(app, "web-1");
  await app.page.getByRole("button", { name: "Overview", exact: true }).click();
  await openSession(app, LONG_NAME);
}

test("screenshots: connect screen, session, MCP session and toasts", async ({ app }, info) => {
  if (info.project.name !== "chromium-large") test.skip();
  await app.launch(longServers());
  const page = app.page;
  await expect(page.getByRole("button", { name: /^db-primary\b/ })).toBeVisible();
  await captureScreen(page, "shell-connect");
  await showToasts(page);
  await captureScreen(page, "shell-toasts");
  await openSeveral(app);
  await captureScreen(page, "shell-session");
  await app.emit("mcp-action-tab-requested", { sessionId: "mcp-session-1", serverId: "lab", clientName: "Claude Desktop" });
  await page.getByRole("button", { name: /Claude Desktop @/ }).first().click();
  await captureScreen(page, "shell-mcp-session");
});

test("header: Tab order follows the visual order, with visible focus", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await expect(page.getByRole("button", { name: /^db-primary\b/ })).toBeVisible();
  await expectTabOrderFollowsVisualOrder(page, page.locator("header", { has: page.locator("nav") }));
});

test("session: tab bar and session header keep Tab order and focus indicators", async ({ app }) => {
  await app.launch(longServers());
  const page = app.page;
  await openSeveral(app);
  // The tab strip and the session header; the chat column and the terminal/files
  // column sit side by side, so each is walked on its own.
  await expectTabOrderFollowsVisualOrder(page, page.locator("header[data-tauri-drag-region]"));
  await expectTabOrderFollowsVisualOrder(page, page.locator("header:visible").last());
  await expectTabOrderFollowsVisualOrder(page, page.getByRole("button", { name: "Terminal", exact: true }).locator(".."));
});

test("session: the tab strip is one Tab stop; arrow keys, Home and End move within it", async ({ app }) => {
  await app.launch(longServers());
  const page = app.page;
  await openSeveral(app);
  const strip = page.getByRole("toolbar");
  const stops = strip.locator("[data-tab-stop]");
  const overview = stops.first();
  await overview.focus();
  await page.keyboard.press("ArrowRight");
  await expect(stops.nth(1)).toBeFocused();
  await page.keyboard.press("End");
  await expect(stops.last()).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(overview).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(stops.last()).toBeFocused();
  await page.keyboard.press("Home");
  await expect(overview).toBeFocused();
  // Only one switch button of the strip is in the Tab order.
  expect(await strip.locator('[data-tab-stop]:not([tabindex="-1"])').count()).toBe(1);
  // Delete on a focused tab asks to close it.
  await stops.nth(1).focus();
  await page.keyboard.press("Delete");
  await expect.poll(async () => (await app.calls("disconnect")).length).toBe(1);
  await expect(stops).toHaveCount(3);
});

test("session: long names stay inside the tab bar and the session header", async ({ app }) => {
  await app.launch(longServers());
  const page = app.page;
  await openSeveral(app);
  await expectNoClippedText(page.locator("#root"));
  await expectUsable(page.getByRole("button", { name: "Disconnect", exact: true }));
});

test("toasts: long messages wrap, actions and dismiss are reachable by keyboard", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await showToasts(page);
  const host = page.getByRole("status");
  await expectNoClippedText(host);
  await expectTabOrderFollowsVisualOrder(page, host);
  await expectNoSeriousA11yViolations(page, "toasts");
});

test("no-provider hint: Settings opened from the link close with Escape and focus returns to it", async ({ app }) => {
  await app.launch({ ...populated(), aiProviders: [] });
  const page = app.page;
  const link = page.getByRole("button", { name: "Set it up now in Settings" });
  await link.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(link).toBeFocused();
});

