// Case 8: session tabs with an MCP-initiated session.
import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { openSession } from "../support/session";

/** The tab (button plus close button) whose label is `label`. */
function tab(page: Page, label: string) {
  return page
    .locator("div.group")
    .filter({ has: page.getByRole("button", { name: `Close tab "${label}"`, exact: true }) });
}

const ACTIVE = /border-t-accent/;

test("an MCP session gets its own tab, and closing the active user tab never activates it", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;

  await openSession(app, "db-primary");
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  const webSession = await openSession(app, "web-1");
  await expect(tab(page, "web-1")).toHaveClass(ACTIVE);

  await app.emit("mcp-action-tab-requested", {
    sessionId: "mcp-session-1",
    serverId: "s-lab",
    clientName: "Claude Desktop",
  });
  const mcpTab = tab(page, "Claude Desktop @ lab-box");
  await expect(mcpTab).toBeVisible();
  await expect(mcpTab.getByText("MCP", { exact: true })).toBeVisible();
  // It opens in the background: the user's tab stays active.
  await expect(mcpTab).not.toHaveClass(ACTIVE);
  await expect(tab(page, "web-1")).toHaveClass(ACTIVE);

  // Close the active user tab: the other user tab becomes active, not the
  // MCP tab (which is the most recently added one).
  await page.getByRole("button", { name: 'Close tab "web-1"', exact: true }).click();
  await expect(tab(page, "web-1")).toHaveCount(0);
  await expect(tab(page, "db-primary")).toHaveClass(ACTIVE);
  await expect(mcpTab).not.toHaveClass(ACTIVE);
  await expect(page.getByText(/This session belongs to an external tool/)).toBeHidden();
  expect((await app.calls("disconnect")).map((c) => c.args.sessionId)).toEqual([webSession]);
});

test("title bar: the version text yields to the tab strip in a narrow window, the Dev marker stays", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await openSession(app, "db-primary");
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await openSession(app, "web-1");

  const full = page.getByTestId("titlebar-version-full");
  const dev = page.getByTestId("titlebar-dev-marker");
  const tabs = page.locator("header div.group").first();
  const narrow = (page.viewportSize()?.width ?? 0) < 1024;

  await expect(tabs).toBeVisible();
  if (narrow) {
    await expect(full).toBeHidden();
    await expect(dev).toBeVisible();
  } else {
    await expect(full).toBeVisible();
    await expect(dev).toBeHidden();
  }

  // Nothing in the title bar overlaps the tab strip.
  const header = page.locator("header").first();
  const overlaps = await header.evaluate((h) => {
    const strip = h.querySelector("div.group")!.closest("div.flex-1")!.getBoundingClientRect();
    const brand = h.querySelector("[data-testid^='titlebar-']")!.closest("div")!.getBoundingClientRect();
    return brand.right > strip.left + 0.5 && brand.left < strip.right - 0.5;
  });
  expect(overlaps, "title-bar brand area overlaps the tab strip area").toBe(false);
});

// Issue #1365: the main navigation stays in the title bar during a session.
test("title bar: navigation tabs stay visible in a session, Connect and + lead back to the overview, no Overview tile", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await openSession(app, "db-primary");

  for (const name of ["Connect", "Manage", "Filter Rules"]) {
    await expect(page.getByRole("button", { name, exact: true })).toBeVisible();
  }
  await expect(page.getByRole("button", { name: "Overview", exact: true })).toHaveCount(0);

  const plus = page.getByRole("button", { name: "New connection", exact: true });
  await plus.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "Connect", exact: true })).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("heading", { name: "Servers" })).toBeVisible();

  await openSession(app, "web-1");
  await page.getByRole("button", { name: "Manage", exact: true }).click();
  await expect(page.getByRole("button", { name: "Manage", exact: true })).toHaveAttribute("aria-current", "page");
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Servers" })).toBeVisible();
});

test("with many open tabs every tab keeps a readable width and the strip scrolls", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await openSession(app, "db-primary");
  for (let i = 1; i <= 5; i++) {
    await app.emit("mcp-action-tab-requested", {
      sessionId: `mcp-session-${i}`,
      serverId: "s-lab",
      clientName: `Client ${i}`,
    });
  }
  const tabs = page.locator("header div.group");
  await expect(tabs).toHaveCount(6);
  for (let i = 0; i < 6; i++) {
    await expect
      .poll(async () => (await tabs.nth(i).locator("button").first().boundingBox())?.width ?? 0)
      .toBeGreaterThan(40);
  }
  const strip = page.getByRole("toolbar").first();
  await expect.poll(() => strip.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  await expect(strip).toHaveAttribute("data-overflow-end", "");
  // A plain vertical mouse wheel scrolls the strip horizontally.
  await strip.hover();
  await page.mouse.wheel(0, 2000);
  await expect.poll(() => strip.evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
  const plus = page.getByRole("button", { name: "New connection", exact: true });
  await expect(plus).toBeInViewport();
  await plus.click();
  await expect(page.getByRole("button", { name: "Connect", exact: true })).toHaveAttribute("aria-current", "page");
  await strip.evaluate((el) => (el.scrollLeft = el.scrollWidth));
  await tabs.last().locator("button").first().click();
  await expect(tabs.last().locator("button").first()).toHaveAttribute("aria-current", "page");
});

test("the tab context menu is not clipped by the fade mask while the strip overflows", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await openSession(app, "db-primary");
  for (let i = 1; i <= 5; i++) {
    await app.emit("mcp-action-tab-requested", { sessionId: `mcp-session-${i}`, serverId: "s-lab", clientName: `Client ${i}` });
  }
  const strip = page.getByRole("toolbar").first();
  await expect.poll(() => strip.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  await expect(strip).toHaveAttribute("data-overflow-end", "");
  await page.locator("header div.group").first().click({ button: "right" });
  const item = page.getByRole("menuitem", { name: "New session" });
  await expect(item).toBeVisible();
  // The menu must live outside the masked strip, otherwise the mask clips it at the strip's bottom edge.
  await expect(strip.getByRole("menuitem", { name: "New session" })).toHaveCount(0);
  await item.click();
  await expect(item).toHaveCount(0);
});
