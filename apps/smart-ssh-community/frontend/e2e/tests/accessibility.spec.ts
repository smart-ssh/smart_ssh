// Case 10: axe scans of the main screen, the settings and the command
// confirmation. Known violations that this issue does not fix are listed,
// with a reason, in `support/axe.ts`.
import { expect, test } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { populated } from "../support/data";
import { openSession, redCommandProposal } from "../support/session";

test.describe("accessibility", () => {
  test("main screen", async ({ app }) => {
    await app.launch(populated());
    await expect(app.page.getByText("db-primary")).toBeVisible();
    await expectNoSeriousA11yViolations(app.page, "main");
  });

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

  test("command confirmation", async ({ app }) => {
    await app.launch(populated());
    const sessionId = await openSession(app, "db-primary");
    await app.emit("chat-action-proposed", redCommandProposal(sessionId, "rm -rf /var/lib/app/cache"));
    await expect(app.page.getByRole("button", { name: "Execute", exact: true })).toBeVisible();
    await expectNoSeriousA11yViolations(app.page, "command confirmation");
  });
});
