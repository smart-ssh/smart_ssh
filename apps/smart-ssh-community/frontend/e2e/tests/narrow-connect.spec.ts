// Issue #582, Spec 0113 §5a: the Connect area (server tree, session picker,
// first-run notice) at phone width. Tagged `@phone`: runs in the phone projects only.
import { expect, test } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { ACKNOWLEDGED, group, populated, server } from "../support/data";
import { expectFullyInViewport, expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";

async function noHorizontalScroll(page: import("@playwright/test").Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
}

test.describe("narrow Connect area @phone", () => {
  test("nested server tree has no horizontal scroll; no row actions, rows are 44 px high", async ({ app }) => {
    await app.launch({
      ...populated(),
      groups: [
        ...populated().groups!,
        group({ id: "g-deep", name: "A very long folder name that keeps going and going", parentId: "g-web" }),
      ],
      servers: [
        ...populated().servers!,
        server({ id: "s-long", name: "an-extremely-long-server-name-for-the-phone-layout", host: "a-very-long-hostname.internal.example.test", groupId: "g-deep", tags: ["one", "two", "three"] }),
      ],
    });
    const page = app.page;
    await noHorizontalScroll(page);
    await expect(page.getByRole("button", { name: /^Move “.*” to…$/ })).toHaveCount(0);
    await expect(page.getByRole("button", { name: /^New server in/ })).toHaveCount(0);
    for (const row of await page.getByRole("button", { name: /^(db-primary|web-1|lab-box|standalone|an-extremely)/ }).all()) {
      expect((await row.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    }
    await expectNoSeriousA11yViolations(page, "narrow server tree");
    await captureScreen(page, "connect-narrow-tree");
  });

  test("servers start at the 📁 icon of their folder (three levels, 28 px step)", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    const pad = async (loc: import("@playwright/test").Locator) =>
      parseFloat(await loc.evaluate((el) => (el as HTMLElement).style.paddingLeft));
    const prod = page.getByRole("button", { name: /^📁 Production/ });
    const web = page.getByRole("button", { name: /^📁 Web/ });
    const web1 = page.getByRole("button", { name: /^web-1\b/ });
    await expect(web1).toBeVisible();
    expect((await prod.locator("svg").boundingBox())!.width).toBe(20);
    expect((await pad(web)) - (await pad(prod))).toBe(28);
    expect((await pad(web1)) - (await pad(web))).toBe(28);
    await noHorizontalScroll(page);
  });

  test("session picker: New conversation and the list are usable; selecting resumes", async ({ app }) => {
    await app.launch({
      settings: { ...ACKNOWLEDGED },
      servers: [server({ id: "s-1", name: "alpha" })],
      chatSessions: {
        "s-1": Array.from({ length: 12 }, (_, i) => ({
          sessionId: `c-${i}`,
          title: i === 0 ? "Check disk usage on the volume holding the database backups" : `Session ${i}`,
          startedAt: "2026-05-01T10:00:00Z",
          endedAt: null,
          messageCount: 3,
        })),
      },
      commands: { resume_chat_session: { hold: true } },
    });
    const page = app.page;
    await page.getByRole("button", { name: /^alpha\b/ }).click();
    const dialog = page.getByRole("dialog");
    await expectFullyInViewport(dialog);
    await expectUsable(dialog.getByRole("button", { name: "New conversation" }));
    await expectUsable(dialog.getByRole("button", { name: "Cancel", exact: true }));
    await expectUsable(dialog.getByRole("button", { name: "Rename" }).first());
    await noHorizontalScroll(page);
    await expectNoSeriousA11yViolations(page, "narrow session picker");
    await captureScreen(page, "connect-narrow-session-picker");
    await dialog.getByRole("button", { name: /Session 1\b/ }).click();
    await expect(page.getByRole("heading", { name: "Connecting to alpha …" })).toBeVisible();
  });

  test("first-run notice fits the width and Continue is usable", async ({ app }) => {
    await app.launch({ servers: [server({ id: "s-1", name: "alpha" })] });
    const page = app.page;
    await page.getByRole("button", { name: /^alpha\b/ }).click();
    const dialog = page.getByRole("dialog");
    await expectFullyInViewport(dialog);
    await expectUsable(dialog.getByRole("checkbox"));
    await expectUsable(dialog.getByRole("button", { name: "Continue", exact: true }));
    await noHorizontalScroll(page);
    await captureScreen(page, "connect-narrow-first-run");
  });
});
