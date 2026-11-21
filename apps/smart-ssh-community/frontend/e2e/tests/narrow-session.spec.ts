import { expect, test } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { populated } from "../support/data";
import { expectNoHorizontalPageScroll, expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";
import { openSession } from "../support/session";

// Issue #580, Spec 0113 §4: the session view shows one pane behind AI / Terminal / Files tabs.
test.describe("narrow session view @phone", () => {
  test("AI is active, panes switch, no page scroll, Disconnect reachable", async ({ app }) => {
    await app.launch(populated());
    await openSession(app, "web-1");
    const page = app.page;
    const tab = (name: string) => page.getByRole("tab", { name, exact: true });
    for (const name of ["AI", "Terminal", "Files"]) {
      await expectUsable(tab(name));
      // Issue #1366: every pane tab carries an icon.
      await expect(tab(name).locator("[data-icon]")).toHaveCount(1);
    }
    // Spec 0113 §4: name and status stay visible in the header.
    await expect(
      page.locator("header").filter({ has: page.getByRole("button", { name: "More actions" }) }).locator("[data-status-square]"),
    ).toBeVisible();
    await expect(tab("AI")).toHaveAttribute("aria-selected", "true");
    await expect(page.getByRole("separator")).toHaveCount(0);
    await captureScreen(page, "session-narrow-ai");

    await tab("Terminal").click();
    await expect(page.locator(".xterm-screen").locator("visible=true")).toBeVisible();
    // Narrow layout: 10 px inner padding on the terminal surface (issue #1370).
    await expect(page.getByTestId("terminal-surface").locator("visible=true")).toHaveCSS("padding-left", "10px");
    await captureScreen(page, "session-narrow-terminal");
    await tab("Files").click();
    await captureScreen(page, "session-narrow-files");
    const bar = page.getByTestId("narrow-action-bar");
    await expectUsable(bar.getByRole("button", { name: "Upload" }));
    await expectUsable(bar.getByRole("button", { name: "+ Folder" }));
    await expectNoSeriousA11yViolations(page, "session narrow files");
    await tab("AI").click();
    await expectNoHorizontalPageScroll(page);

    const more = page.getByRole("button", { name: "More actions" });
    await expectUsable(more);
    await more.click();
    await expectUsable(page.getByRole("menuitem", { name: "Disconnect" }));
    await expectNoSeriousA11yViolations(page, "session narrow");
  });
});
