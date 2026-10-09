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

const ACTIVE = /bg-indigo-600\/16/;

test("an MCP session gets its own tab, and closing the active user tab never activates it", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;

  await openSession(app, "db-primary");
  await page.getByRole("button", { name: "Overview", exact: true }).click();
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
