// Issue #585, Spec 0113 §5/§6: the Manage area (list → detail) at phone width.
import { expect, test, type AppDriver } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { ACKNOWLEDGED, group, populated, server } from "../support/data";
import { expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";

async function noHorizontalScroll(page: import("@playwright/test").Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
}

async function openManage(app: AppDriver) {
  const page = app.page;
  await page.getByRole("button", { name: "Open navigation menu" }).click();
  await page.getByRole("dialog", { name: "Navigation menu" }).getByRole("button", { name: "Manage", exact: true }).click();
}

const IMPORT_PREVIEW = {
  groups: [{ name: "config", parent: null, sourcePath: "/home/me/.ssh/config" }],
  entries: Array.from({ length: 4 }, (_, index) => ({
    index,
    name: index === 0 ? "very-long-server-name-".repeat(6) : `host-${index}`,
    group: 0,
    host: { value: `host-${index}.example.test`, origin: null },
    port: { value: 22, origin: null },
    username: { value: "deploy", origin: null },
    tags: [],
    identityFile: null,
    jumpHost: null,
    conflict: null,
  })),
  files: [{ path: "/home/me/.ssh/config", depth: 0, status: "read" }],
  skipped: [],
  identityFilePaths: [],
};

const back = (app: AppDriver) => app.page.getByRole("button", { name: /Back/ });

test.describe("narrow Manage area @phone", () => {
  test("list is full width; a server opens its form; Back returns to the list", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    await openManage(app);
    const row = page.getByRole("button", { name: "🖥️ db-primary" });
    await expectUsable(row);
    expect((await row.boundingBox())!.width).toBeGreaterThan(300);
    await expectUsable(page.getByRole("button", { name: "Import …", exact: true }));
    await expectUsable(page.getByRole("button", { name: "Export …", exact: true }));
    await expect(page.getByText("Select a group or server on the left")).toHaveCount(0);
    await noHorizontalScroll(page);
    await expectNoSeriousA11yViolations(page, "narrow manage list");
    await captureScreen(page, "manage-narrow-list");

    await row.click();
    await expect(page.getByRole("button", { name: "🖥️ db-primary" })).toHaveCount(0);
    await expectUsable(back(app));
    await expectUsable(page.getByRole("button", { name: "Save", exact: true }));
    await noHorizontalScroll(page);
    await expectNoSeriousA11yViolations(page, "narrow server form");
    await captureScreen(page, "manage-narrow-server-form");
    await back(app).click();
    await expect(page.getByRole("button", { name: "🖥️ db-primary" })).toBeVisible();
  });

  test("Back keeps the list scroll position", async ({ app }) => {
    const many = Array.from({ length: 40 }, (_, i) => server({ id: `s-${i}`, name: `server-${String(i).padStart(2, "0")}` }));
    await app.launch({ settings: { ...ACKNOWLEDGED }, servers: many });
    const page = app.page;
    await openManage(app);
    const row = page.getByRole("button", { name: "🖥️ server-35" });
    await row.scrollIntoViewIfNeeded();
    const scroller = page.locator('[data-drop-target="root"]');
    const before = await scroller.evaluate((el) => el.scrollTop);
    expect(before).toBeGreaterThan(0);
    await row.click();
    await back(app).click();
    await expect.poll(() => scroller.evaluate((el) => el.scrollTop)).toBe(before);
  });

  test("Move to… is reachable from the list rows", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    await openManage(app);
    await page.getByRole("button", { name: "Move “standalone” to…" }).click();
    await page.getByRole("dialog").getByRole("button", { name: /Lab/ }).click();
    await expect.poll(() => app.calls("move_server_to_group")).toHaveLength(1);
  });

  test("group form, import and export fit", async ({ app }) => {
    await app.launch({ ...populated(), commands: { preview_ssh_config_import: { result: IMPORT_PREVIEW } }, groups: [...populated().groups!, group({ id: "g-x", name: "Group with a very long name ".repeat(4) })] });
    const page = app.page;
    await openManage(app);
    await page.getByRole("button", { name: /📁 Group with a very long name/ }).click();
    await expectUsable(back(app));
    await expectUsable(page.getByRole("button", { name: "Save", exact: true }));
    await noHorizontalScroll(page);
    await expectNoSeriousA11yViolations(page, "narrow group form");
    await captureScreen(page, "manage-narrow-group-form");
    await back(app).click();
    await page.getByRole("button", { name: "Import …", exact: true }).click();
    await expectUsable(page.getByRole("dialog"));
    await noHorizontalScroll(page);
    await expectNoSeriousA11yViolations(page, "narrow import");
    await captureScreen(page, "manage-narrow-import");
  });

  test("export dialog, notes and a group-form extension section fit", async ({ app }) => {
    await app.launch({ ...populated(), commands: { export_ssh_config: { result: { path: "/home/me/very/long/path/".repeat(5) + "config", exported: [{ originalName: "db primary", alias: "db-primary", renamed: true }], includeHint: "Include /home/me/very/long/path/".repeat(3) } } } });
    const page = app.page;
    await page.evaluate(() => {
      const ext = (window as unknown as { __e2eExtensions: Record<string, (...a: unknown[]) => unknown> }).__e2eExtensions;
      const h = ext.createElement;
      const long = "Group extension content that is long enough to need wrapping on a phone. ".repeat(4);
      ext.registerGroupFormSection({ id: "ext-group-section", component: () => h("p", { "data-testid": "ext-group-section" }, long) });
    });
    await openManage(app);
    await page.getByRole("button", { name: "Export …", exact: true }).click();
    await expectUsable(page.getByRole("dialog"));
    await noHorizontalScroll(page);
    await expectNoSeriousA11yViolations(page, "narrow export");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);

    await page.getByRole("button", { name: "📁 Lab" }).click();
    const groupSection = page.getByTestId("ext-group-section");
    await groupSection.scrollIntoViewIfNeeded();
    await expectUsable(groupSection);
    await noHorizontalScroll(page);
    await back(app).click();

    await page.getByRole("button", { name: "🖥️ db-primary" }).click();
    const notes = page.getByText("Notes", { exact: true }).first();
    await notes.scrollIntoViewIfNeeded();
    await expectUsable(notes);
    await noHorizontalScroll(page);
    await expectNoSeriousA11yViolations(page, "narrow notes");
  });

  test("extension manage view and server form section are reachable", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    await page.evaluate(() => {
      const ext = (window as unknown as { __e2eExtensions: Record<string, (...a: unknown[]) => unknown> }).__e2eExtensions;
      const h = ext.createElement;
      const long = "Extension content that is long enough to need wrapping on a phone. ".repeat(4);
      ext.registerManageView({ id: "ext-view", label: "Extension view", component: () => h("p", { className: "p-4" }, long) });
      ext.registerServerFormSection({ id: "ext-section", component: () => h("p", { "data-testid": "ext-section" }, long) });
    });
    await openManage(app);
    await page.getByRole("button", { name: /Extension view/ }).click();
    await expectUsable(back(app));
    await noHorizontalScroll(page);
    await back(app).click();
    await expectUsable(page.getByRole("button", { name: /Extension view/ }));
    await page.getByRole("button", { name: "🖥️ db-primary" }).click();
    const section = page.getByTestId("ext-section");
    await section.scrollIntoViewIfNeeded();
    await expectUsable(section);
    await noHorizontalScroll(page);
  });
});
