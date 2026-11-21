// Issue #1381: review screenshots of the server form frame, General, Security and Notes tabs and the group form.
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { captureScreen } from "../support/screenshots";

for (const [tab, name] of [["General", "server-form-general"], ["Security", "server-form-security"], ["Notes", "server-form-notes"]] as const) {
  test(`${tab} tab screenshots`, async ({ app }) => {
    await app.launch(populated());
    await app.page.getByRole("button", { name: "Manage", exact: true }).click();
    await app.page.getByRole("treeitem", { name: /^web-1\b/ }).click();
    await app.page.getByRole("tab", { name, exact: false }).or(app.page.getByRole("tab", { name: tab, exact: true })).first().click();
    await expect(app.page.getByRole("tabpanel").filter({ visible: true }).first()).toBeVisible();
    await captureScreen(app.page, name);
  });
}
