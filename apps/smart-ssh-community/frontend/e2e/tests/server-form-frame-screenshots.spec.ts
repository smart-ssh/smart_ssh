// Issue #1381: review screenshots of the server form frame, General, Security and Notes tabs and the group form.
import { expect, test } from "../support/app";
import { group, populated, server } from "../support/data";
import { NOTE_COMMANDS, openNotes } from "../support/notes";
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

test("Security tab with stored host keys screenshots", async ({ app }) => {
  await app.launch({
    ...populated(),
    commands: {
      list_stored_host_keys: {
        result: [
          { algorithm: "ssh-ed25519", fingerprint: "SHA256:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG" },
          { algorithm: "ssh-rsa", fingerprint: "SHA256:ZYXWVUTSRQPONMLKJIHGFEDCBA9876543210gfedcba" },
        ],
      },
    },
  });
  await app.page.getByRole("button", { name: "Manage", exact: true }).click();
  await app.page.getByRole("treeitem", { name: /^web-1\b/ }).click();
  await app.page.getByRole("tab", { name: "Security", exact: true }).click();
  const lastKey = app.page.getByText("ssh-rsa", { exact: true });
  await lastKey.scrollIntoViewIfNeeded();
  await expect(lastKey).toBeVisible();
  await captureScreen(app.page, "server-form-security-host-keys");
});

test("Group form screenshots", async ({ app }) => {
  await app.launch({ ...populated(), groups: [...populated().groups!, group({ id: "g-x", name: "Staging" })] });
  await app.page.getByRole("button", { name: "Manage", exact: true }).click();
  await app.page.getByRole("treeitem", { name: /Staging/ }).or(app.page.getByRole("button", { name: /^Staging/ })).first().click();
  await expect(app.page.getByRole("button", { name: "Save", exact: true })).toBeVisible();
  await captureScreen(app.page, "group-form");
});

test("Notes tab with expanded history screenshots", async ({ app }) => {
  await app.launch({ ...populated(), commands: NOTE_COMMANDS });
  await openNotes(app);
  await app.page.getByRole("button", { name: "Show history", exact: true }).click();
  await expect(app.page.getByRole("button", { name: "Restore" }).first()).toBeVisible();
  await captureScreen(app.page, "server-form-notes-history");
});

test("Local server form screenshots", async ({ app }) => {
  await app.launch({ ...populated(), servers: [...populated().servers!, server({ id: "s-local", name: "This computer", isLocal: true })] });
  await app.page.getByRole("button", { name: "Manage", exact: true }).click();
  await app.page.getByRole("treeitem", { name: /^This computer/ }).or(app.page.getByRole("button", { name: /^This computer/ })).first().click();
  await expect(app.page.getByRole("button", { name: "Save", exact: true })).toBeVisible();
  await captureScreen(app.page, "server-form-local");
});
