// Case 10: axe scans of the main screen, the settings and the command
// confirmation. Known violations that this issue does not fix are listed,
// with a reason, in `support/axe.ts`.
import { expect, test, type AppDriver } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { populated } from "../support/data";
import { NOTE_COMMANDS, openNotes } from "../support/notes";
import { openSession, redCommandProposal } from "../support/session";

// New screens: add one line here. `open` brings the app into the state
// to scan; the key is the screen name used in the axe allowlist.
const SCREENS: Record<string, (app: AppDriver) => Promise<void>> = {
  main: async (app) => {
    await app.launch(populated());
    await expect(app.page.getByText("db-primary")).toBeVisible();
  },
  "settings: third-party licenses dialog": async (app) => {
    await app.launch(populated());
    await app.page.getByRole("button", { name: "Settings", exact: true }).click();
    await app.page.getByRole("button", { name: "About", exact: true }).click();
    await app.page.getByRole("button", { name: "Third-party licenses", exact: true }).click();
    await expect(app.page.getByRole("heading", { name: "Third-party licenses" })).toBeVisible();
  },
  "notes panel": async (app) => {
    await app.launch({ ...populated(), commands: NOTE_COMMANDS });
    await openNotes(app);
    await app.page.getByRole("button", { name: "Show history", exact: true }).click();
    await expect(app.page.getByRole("button", { name: "Restore" }).first()).toBeVisible();
  },
  "filter rules": async (app) => {
    await app.launch({
      ...populated(),
      commands: {
        list_rules: {
          result: [
            { id: "r1", patternType: "glob", patternValue: "ls *", action: "Allow", scope: "Global", priority: 10, patternError: null },
            { id: "r2", patternType: "glob", patternValue: "rm *", action: "Deny", scope: "Global", priority: 20, patternError: null },
            { id: "r3", patternType: "exact", patternValue: "reboot", action: "Confirm", scope: "Global", priority: 5, patternError: null },
          ],
        },
        list_hard_blacklist: { result: [{ kind: "regex", value: "rm -rf /" }] },
      },
    });
    await app.page.getByRole("button", { name: "Filter Rules", exact: true }).click();
    await expect(app.page.getByText("ls *")).toBeVisible();
  },
  "command confirmation": async (app) => {
    await app.launch(populated());
    const sessionId = await openSession(app, "db-primary");
    await app.emit("chat-action-proposed", redCommandProposal(sessionId, "rm -rf /var/lib/app/cache"));
    await expect(app.page.getByRole("button", { name: "Execute", exact: true })).toBeVisible();
  },
};

test.describe("accessibility", () => {
  for (const [name, open] of Object.entries(SCREENS)) {
    test(name, async ({ app }) => {
      await open(app);
      await expectNoSeriousA11yViolations(app.page, name);
    });
  }

  test("settings, every category", async ({ app }) => {
    test.setTimeout(90_000);
    await app.launch(populated());
    const page = app.page;
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    const categories = page.locator("nav ul button");
    const count = await categories.count();
    expect(count).toBeGreaterThan(3);
    for (let i = 0; i < count; i++) {
      const category = categories.nth(i);
      await category.click();
      await expect(category).toHaveAttribute("aria-current", "page");
      await expectNoSeriousA11yViolations(page, `settings: ${await category.innerText()}`);
    }
  });
});
