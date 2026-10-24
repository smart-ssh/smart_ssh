// Issue #666, Spec 0109 (K2, K5, X1, I3, I8, T3): "Data and backup" —
// export with the one-time recovery code, then restore with the counts
// confirmation and the summary.
import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { populated } from "../support/data";

async function openBackupSection(page: Page) {
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Data and backup", exact: true }).click();
  await expect(dialog.getByRole("heading", { name: "Encrypted backup" })).toBeVisible();
  return dialog;
}

test.describe("encrypted backup", () => {
  test("first export shows the code once, then export and restore", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    const settings = await openBackupSection(page);
    await expect(settings.getByText(/Not included: chat history/)).toBeVisible();

    // K2: key setup, code shown, Continue only after the confirmation.
    await settings.getByRole("button", { name: "Export backup…" }).click();
    await page.getByRole("button", { name: "Create a recovery code" }).click();
    const code = page.getByRole("dialog", { name: "Your recovery code" });
    await expect(code.getByTestId("backup-recovery-code")).toContainText("ABCDE");
    const next = code.getByRole("button", { name: "Continue" });
    await expect(next).toBeDisabled();
    await code.getByRole("checkbox", { name: "I have noted the code" }).check();
    await expect(next).toBeEnabled();
    await next.click();
    await expect(settings.getByRole("status")).toContainText("smart-ssh-backup.sshbackup");

    // The second export needs no input and does not show a code again.
    await settings.getByRole("button", { name: "Export backup…" }).click();
    await expect(settings.getByRole("status")).toContainText("Backup saved to");
    await expect(page.getByRole("dialog", { name: "Your recovery code" })).toHaveCount(0);

    // Restore: wrong code first, then the counts confirmation, then the summary.
    await settings.getByRole("button", { name: "Restore from backup…" }).click();
    const restore = page.getByRole("dialog", { name: "Restore from backup" });
    await restore.getByRole("textbox", { name: "Recovery code" }).fill("wrong");
    await restore.getByRole("button", { name: "Open backup" }).click();
    await expect(restore.getByText("Wrong code or passphrase, or the file is damaged.")).toBeVisible();
    await restore.getByRole("textbox", { name: "Recovery code" }).fill("TYPO");
    await restore.getByRole("button", { name: "Open backup" }).click();
    await expect(restore.getByText("The code contains a typo.")).toBeVisible();
    await restore.getByRole("textbox", { name: "Recovery code" }).fill("ABCDE-FGHJK");
    await restore.getByRole("button", { name: "Open backup" }).click();

    const confirm = page.getByRole("alertdialog", { name: "Replace your data?" });
    await expect(confirm.getByRole("row", { name: /Rules\s+0\s+42/ })).toBeVisible();
    await confirm.getByRole("button", { name: "Replace data" }).click();
    const summary = page.getByRole("dialog", { name: "Backup restored" });
    await expect(summary).toContainText("Previous data 2026-01-01");
    await expect(summary).toContainText("smart-ssh-before-import.sshbackup");
  });

  test("cancelling the confirmation changes nothing", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    const settings = await openBackupSection(page);
    await settings.getByRole("button", { name: "Restore from backup…" }).click();
    const restore = page.getByRole("dialog", { name: "Restore from backup" });
    await restore.getByRole("textbox", { name: "Recovery code" }).fill("ABCDE-FGHJK");
    await restore.getByRole("button", { name: "Open backup" }).click();
    await page.getByRole("alertdialog", { name: "Replace your data?" }).getByRole("button", { name: "Cancel" }).click();
    expect(await app.calls("confirm_backup_import")).toHaveLength(0);
    expect(await app.calls("cancel_backup_import")).toHaveLength(1);
  });

  test("passphrase path rejects short and mismatching input", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    const settings = await openBackupSection(page);
    await settings.getByRole("button", { name: "Set passphrase" }).click();
    const dialog = page.getByRole("dialog", { name: "Set a passphrase" });
    await expect(dialog.getByText(/cannot be read/)).toBeVisible();
    const save = dialog.getByRole("button", { name: "Save passphrase" });
    await dialog.getByLabel("Passphrase", { exact: true }).fill("short");
    await expect(dialog.getByText(/Too short/)).toBeVisible();
    await expect(save).toBeDisabled();
    await dialog.getByLabel("Passphrase", { exact: true }).fill("a long enough passphrase");
    await dialog.getByLabel("Repeat passphrase").fill("something else entirely");
    await expect(dialog.getByText("The passphrases do not match.")).toBeVisible();
    await expect(save).toBeDisabled();
    await dialog.getByLabel("Repeat passphrase").fill("a long enough passphrase");
    await expect(save).toBeEnabled();
  });
});

test.describe("encrypted backup @phone", () => {
  test("section and dialogs fit the narrow window with touch-sized buttons", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    await page.getByRole("button", { name: "Open navigation menu" }).click();
    await page.getByRole("dialog", { name: "Navigation menu" }).getByRole("button", { name: "Settings", exact: true }).click();
    const settings = page.getByRole("dialog", { name: "Settings" });
    await settings.getByRole("button", { name: "Data and backup", exact: true }).click();
    for (const name of ["Export backup…", "Restore from backup…", "New recovery code", "Set passphrase"]) {
      const box = await settings.getByRole("button", { name }).boundingBox();
      expect(box!.height).toBeGreaterThanOrEqual(43.5);
      expect(box!.x + box!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
    }
    await settings.getByRole("button", { name: "Export backup…" }).click();
    const setup = page.getByRole("dialog", { name: "Protect your backups" });
    const dbox = await setup.boundingBox();
    expect(dbox!.width).toBeGreaterThanOrEqual(page.viewportSize()!.width - 1);
  });
});
