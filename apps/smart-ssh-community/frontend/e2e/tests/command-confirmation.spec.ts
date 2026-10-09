// Case 3: a command that needs confirmation, long and multi-line, with a
// red risk. Everything the user decides on must be visible and reachable,
// even in the small window, and each decision reaches the backend once.
import type { Locator } from "@playwright/test";
import { expect, test, type AppDriver } from "../support/app";
import { populated } from "../support/data";
import { expectReachableByTab, expectUsable } from "../support/layout";
import { openSession, redCommandProposal } from "../support/session";

const LONG_COMMAND = [
  "set -euo pipefail",
  ...Array.from(
    { length: 30 },
    (_, i) => `rm -rf /var/lib/app/cache/shard-${i} && echo "cleared shard ${i}"`,
  ),
  `tar czf /tmp/backup.tgz ${"/srv/data/very/long/path/segment ".repeat(12).trim()}`,
].join("\n");

async function proposeLongRedCommand(app: AppDriver) {
  await app.launch(populated());
  const sessionId = await openSession(app, "db-primary");
  await app.emit("chat-action-proposed", redCommandProposal(sessionId, LONG_COMMAND));
  // The action card: the bordered block in the chat that holds the buttons.
  const card = app.page
    .locator("div.border.p-3.text-sm")
    .filter({ has: app.page.getByRole("button", { name: "Execute", exact: true }) })
    .filter({ visible: true });
  await expect(card).toBeVisible();
  return { sessionId, card };
}

function decisionButtons(card: Locator) {
  return {
    confirm: card.getByRole("button", { name: "Execute", exact: true }),
    reject: card.getByRole("button", { name: "Deny", exact: true }),
  };
}

test.describe("command confirmation", () => {
  test("shows the full command, the risk and both usable decision buttons", async ({ app }) => {
    const { card } = await proposeLongRedCommand(app);

    // The full command: every line is in the editable field, which scrolls.
    const editor = card.locator("textarea");
    await expect(editor).toHaveValue(LONG_COMMAND);
    const scroll = await editor.evaluate((el) => ({
      overflowY: getComputedStyle(el).overflowY,
      scrollable: el.scrollHeight > el.clientHeight,
    }));
    expect(scroll.scrollable, "the long command must overflow its field").toBe(true);
    expect(["auto", "scroll"]).toContain(scroll.overflowY);

    // The risk level, on both axes, in the red style.
    for (const axis of ["Server", "Data"]) {
      const badge = card.getByText(axis, { exact: true });
      await expect(badge).toBeVisible();
      await expect(badge).toHaveClass(/bg-red-900/);
    }

    const { confirm, reject } = decisionButtons(card);
    await expectUsable(confirm);
    await expectUsable(reject);

    // Keyboard: from the command field, Tab reaches both buttons.
    await editor.focus();
    await expectReachableByTab(app.page, confirm);
    await expectReachableByTab(app.page, reject);
  });

  test("Deny sends exactly one deny decision", async ({ app }) => {
    const { sessionId, card } = await proposeLongRedCommand(app);
    await decisionButtons(card).reject.click();
    await expect(decisionButtons(card).reject).toHaveCount(0);
    expect(await app.calls("respond_to_action")).toEqual([
      {
        cmd: "respond_to_action",
        args: { sessionId, actionId: "action-1", decision: { decision: "deny" } },
      },
    ]);
  });

  test("Execute sends exactly one approve decision", async ({ app }) => {
    const { sessionId, card } = await proposeLongRedCommand(app);
    await decisionButtons(card).confirm.click();
    await expect(decisionButtons(card).confirm).toHaveCount(0);
    expect(await app.calls("respond_to_action")).toEqual([
      {
        cmd: "respond_to_action",
        args: { sessionId, actionId: "action-1", decision: { decision: "approve" } },
      },
    ]);
  });
});
