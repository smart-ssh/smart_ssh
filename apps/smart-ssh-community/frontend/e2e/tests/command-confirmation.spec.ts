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
      const badge = card.getByText(new RegExp(`^${axis}ⓘ?$`));
      await expect(badge).toBeVisible();
      await expect(badge).toHaveClass(/bg-danger-subtle-hover/);
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
    await expect.poll(() => app.calls("respond_to_action")).toEqual([
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
    await expect.poll(() => app.calls("respond_to_action")).toEqual([
      {
        cmd: "respond_to_action",
        args: { sessionId, actionId: "action-1", decision: { decision: "approve" } },
      },
    ]);
  });

  // Issue #1357: rules per sub-command, staged and saved in one batch.
  test("stages a rule per sub-command, saves them without executing, then Execute runs", async ({ app }) => {
    // One canned answer for every row: it offers both rules.
    const offer = (tool: string, pattern: string) => ({
      kind: "tool", names: [tool], patterns: [{ patternType: "glob", patternValue: pattern }],
      segmentVerdict: { covered: true, reasons: [] }, commandVerdict: { autoExec: false, reasons: [] },
    });
    await app.launch({
      ...populated(),
      commands: {
        explain_confirmation: { result: { segments: [
          { index: 0, text: "echo a", state: "covered", rule: { patternType: "glob", patternValue: "echo *" }, reasons: [] },
          { index: 1, text: "ls -la /tmp", state: "needsRule", rule: null, reasons: [] },
          { index: 2, text: "git status", state: "needsRule", rule: null, reasons: [] },
        ], suggestions: [] } },
        explain_segment_rules: { result: {
          segmentIndex: 1, segmentText: "ls -la /tmp", blockedReasons: [],
          suggestions: [offer("ls", "ls *"), offer("git", "git status *")],
        } },
        create_quick_rules_batch: { result: ["r1", "r2"] },
      },
    });
    const sessionId = await openSession(app, "db-primary");
    await app.emit("chat-action-proposed", redCommandProposal(sessionId, "echo a && ls -la /tmp && git status"));
    const page = app.page;
    await expect(page.getByText("Already allowed")).toBeVisible();
    const lsToggle = page.getByRole("button", { name: "Rule suggestions for ls -la /tmp" });
    const gitToggle = page.getByRole("button", { name: "Rule suggestions for git status" });
    await expect(lsToggle).toHaveAttribute("aria-expanded", "false");
    await lsToggle.click();
    await expect(lsToggle).toHaveAttribute("aria-expanded", "true");
    await expect(page.getByRole("radio", { name: "This Server" })).toBeChecked();
    await page.getByRole("button", { name: /All `ls` calls/ }).click();
    await gitToggle.click();
    await page.getByRole("radio", { name: "Global" }).check();
    await page.getByRole("button", { name: /All `git` calls/ }).click();
    await expect(page.getByTestId("staged-rule")).toHaveCount(2);
    await expect(page.getByText("Rules not saved: 2")).toBeVisible();
    await expect.poll(() => app.calls("respond_to_action")).toHaveLength(0);

    await page.getByRole("button", { name: "Save rules (2)" }).click();
    await expect(page.getByTestId("saved-rule")).toHaveCount(2);
    await expect.poll(() => app.calls("create_quick_rules_batch")).toHaveLength(1);
    const [batch] = await app.calls("create_quick_rules_batch");
    expect((batch.args as { rules: { scope: unknown }[] }).rules.map((r) => r.scope)).toEqual([
      { Server: expect.any(String) },
      "Global",
    ]);
    await expect(lsToggle).toBeVisible();
    await expect.poll(() => app.calls("respond_to_action")).toHaveLength(0);
    await page.getByRole("button", { name: "Execute", exact: true }).click();
    await expect.poll(() => app.calls("respond_to_action")).toHaveLength(1);
  });
});
