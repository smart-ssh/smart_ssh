// Issue #1381: server form frame at phone width (Save + Connect row, equal-width tabs, host-key rows).
import { expect, test, type AppDriver } from "../support/app";
import { ACKNOWLEDGED, server } from "../support/data";
import { expectNoHorizontalPageScroll } from "../support/layout";

async function openForm(app: AppDriver) {
  const page = app.page;
  await page.getByRole("button", { name: "Open navigation menu", exact: true }).click();
  await page.getByRole("dialog", { name: "Navigation menu" }).getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("button", { name: "alpha", exact: true }).click();
  await expect(page.getByTestId("server-form")).toBeVisible();
}

test.describe("narrow server form @phone", () => {
  test("Save + Connect row: Connect is disabled with a visible hint while dirty and connects after save", async ({ app }) => {
    await app.launch({
      settings: { ...ACKNOWLEDGED },
      servers: [server({ id: "s-1", name: "alpha" })],
      commands: { update_server: { result: null } },
    });
    const page = app.page;
    await openForm(app);
    const form = page.getByTestId("server-form");
    const connect = form.getByRole("button", { name: "Connect", exact: true });
    const save = form.getByRole("button", { name: "Save", exact: true });
    await expect(connect).toBeEnabled();
    await expect(form.getByTestId("connect-save-first-hint")).toHaveCount(0);
    // Connect sits next to Save, not in the title row.
    const saveBox = (await save.boundingBox())!;
    const connectBox = (await connect.boundingBox())!;
    expect(Math.abs(saveBox.y - connectBox.y)).toBeLessThan(20);
    await expect(connect.locator('svg[data-icon="plug"]')).toBeVisible();

    await page.getByLabel("Name", { exact: true }).first().fill("alpha2");
    await expect(connect).toBeDisabled();
    await expect(form.getByTestId("connect-save-first-hint")).toBeVisible();
    await expect(form.getByTestId("connect-save-first-hint")).toHaveText("Save your changes first");
    // Really rendered (not sr-only): a real box on its own row below Save + Connect.
    const hintBox = (await form.getByTestId("connect-save-first-hint").boundingBox())!;
    const saveBox2 = (await save.boundingBox())!;
    expect(hintBox.width).toBeGreaterThan(40);
    expect(hintBox.height).toBeGreaterThan(8);
    expect(hintBox.y).toBeGreaterThanOrEqual(saveBox2.y + saveBox2.height - 1);
    await expectNoHorizontalPageScroll(page);

    await save.click();
    await expect(connect).toBeEnabled();
    await expect(form.getByTestId("connect-save-first-hint")).toHaveCount(0);
    await connect.click();
    await expect.poll(async () => (await app.calls("connect")).length).toBe(1);
  });

  test("tabs share the row in equal widths and stored host keys show the fingerprint under the algorithm", async ({ app }) => {
    await app.launch({
      settings: { ...ACKNOWLEDGED },
      servers: [server({ id: "s-1", name: "alpha" })],
      commands: {
        list_stored_host_keys: { result: [{ algorithm: "ssh-ed25519", fingerprint: "SHA256:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG" }] },
      },
    });
    const page = app.page;
    await openForm(app);
    const tabs = page.getByRole("tab");
    const count = await tabs.count();
    expect(count).toBeGreaterThanOrEqual(3);
    const widths: number[] = [];
    for (let i = 0; i < count; i++) {
      const box = (await tabs.nth(i).boundingBox())!;
      widths.push(box.width);
      expect(box.height).toBeGreaterThanOrEqual(44);
    }
    for (const w of widths) expect(Math.abs(w - widths[0])).toBeLessThan(2);

    await page.getByRole("tab", { name: "Security", exact: true }).click();
    const algorithm = page.getByText("ssh-ed25519", { exact: true });
    const fingerprint = page.getByText(/^SHA256:abcdef/);
    await expect(algorithm).toBeVisible();
    await expect(fingerprint).toBeVisible();
    const a = (await algorithm.boundingBox())!;
    const f = (await fingerprint.boundingBox())!;
    expect(f.y).toBeGreaterThan(a.y + a.height - 1);
    await expectNoHorizontalPageScroll(page);
  });
});
