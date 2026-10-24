// Issue #581, Spec 0113 §5: dialogs, host key prompt and start-up screens at
// phone width. Tagged `@phone`: runs in the phone projects only.
import { expect, test } from "../support/app";
import type { Locator, Page } from "@playwright/test";
import type { RemoteEntryDto, SshConfigImportPreviewDto } from "../../src/types";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { ACKNOWLEDGED, populated, server } from "../support/data";
import { expectFullyInViewport, expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";

const FINGERPRINT = "SHA256:Zm9vYmFyYmF6cXV4cXV1eGNvcmdlZ3JhdWx0Z2FycGx5";

async function noHorizontalScroll(page: import("@playwright/test").Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
}

test("host key prompt is full-screen with the whole fingerprint and both actions @phone", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-new", name: "fresh-host", host: "fresh.example.test", port: 2222 })],
    hostKeys: { "s-new": { kind: "unknown", fingerprint: FINGERPRINT, keyType: "ssh-ed25519" } },
  });
  const page = app.page;
  await page.getByRole("button", { name: /^fresh-host\b/ }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(FINGERPRINT);
  await expectFullyInViewport(dialog);
  const box = await dialog.boundingBox();
  expect(box!.width).toBeGreaterThanOrEqual(page.viewportSize()!.width - 1);
  await expectUsable(dialog.getByRole("button", { name: "Reject", exact: true }));
  await expectUsable(dialog.getByRole("button", { name: "Trust", exact: true }));
  await noHorizontalScroll(page);
});

test("unlock screen fits without horizontal scroll @phone", async ({ app }) => {
  await app.launch({
    commands: {
      get_startup_state: {
        result: { screen: "unlock", mode: "password", offersStartOver: true, failedUnlockAttempts: 0, language: "en" },
      },
    },
  });
  const page = app.page;
  await expectUsable(page.getByRole("button", { name: "Unlock", exact: true }));
  await noHorizontalScroll(page);
});

const UNLOCK_STATE = {
  screen: "unlock",
  mode: "password",
  offersStartOver: true,
  failedUnlockAttempts: 0,
  language: "en",
};

/** The dialog is a full-screen sheet; its title and the named buttons are usable without scrolling the page. */
async function expectSheet(page: Page, dialog: Locator, title: RegExp | string, buttons: string[]) {
  await expect(dialog).toBeVisible();
  await expect.poll(async () => {
    const box = await dialog.boundingBox();
    const v = page.viewportSize()!;
    return !!box && box.width >= v.width - 1 && box.height >= v.height - 56; // the title-bar strip stays free above the sheet
  }, { message: "dialog fills the viewport" }).toBe(true);
  await expectUsable(dialog.getByRole("heading", { name: title }));
  for (const name of buttons) await expectUsable(dialog.getByRole("button", { name, exact: true }));
  await noHorizontalScroll(page);
}

/** Navigation lives behind the burger menu at phone width (Spec 0113 §3). */
async function openFromMenu(page: Page, name: string) {
  await page.getByRole("button", { name: "Open navigation menu" }).click();
  await page.getByRole("dialog", { name: "Navigation menu" }).getByRole("button", { name, exact: true }).click();
}

const entry = (name: string): RemoteEntryDto => ({
  name,
  path: `/home/deploy/${name}`,
  isDir: false,
  size: 2048,
  permissions: "rw-r--r--",
  modified: "2026-01-15T10:30:00Z",
  permissionsOctal: 0o644,
  uid: 1000,
  gid: 1000,
  owner: "deploy",
  group: "deploy",
});

async function openEntryMenuItem(app: import("../support/app").AppDriver, item: RegExp | string) {
  await app.launch({ ...populated(), commands: { sftp_list: { result: [entry("notes.txt")] } } });
  const page = app.page;
  await page.getByRole("button", { name: /^db-primary\b/ }).click();
  await expect.poll(async () => (await app.calls("connect")).length).toBe(1);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await expect(page.getByText("notes.txt")).toBeVisible();
  await page.getByRole("button", { name: /^Actions: notes.txt$/ }).click();
  await page.getByRole("button", { name: item, exact: typeof item === "string" }).click();
}

test("sm: file properties dialog is a full-screen sheet @phone", async ({ app }) => {
  await openEntryMenuItem(app, "Properties");
  const dialog = app.page.getByRole("dialog");
  await expectSheet(app.page, dialog, /Properties/, ["Close"]);
  await expectNoSeriousA11yViolations(app.page, "narrow: properties");
  await captureScreen(app.page, "narrow-properties");
});

test("sm: chmod dialog is a full-screen sheet @phone", async ({ app }) => {
  await openEntryMenuItem(app, /^Edit permissions|^Permissions/);
  const dialog = app.page.getByRole("dialog");
  await expectSheet(app.page, dialog, /ermissions/, ["Cancel"]);
  await expectNoSeriousA11yViolations(app.page, "narrow: chmod");
  await captureScreen(app.page, "narrow-chmod");
});

test("sm: master password dialog is a full-screen sheet @phone", async ({ app }) => {
  await app.launch({ commands: { get_startup_state: { result: UNLOCK_STATE } } });
  await app.emit("startup:prompt", { kind: "newMasterPassword", title: "Set a master password", message: "Choose one." });
  const dialog = app.page.getByRole("dialog");
  await expectSheet(app.page, dialog, "Set a master password", ["Cancel", "Yes, continue"]);
  await expectNoSeriousA11yViolations(app.page, "narrow: new master password");
  await captureScreen(app.page, "narrow-master-password");
});

test("md: first-run notice is a full-screen sheet @phone", async ({ app }) => {
  await app.launch({ servers: [server({ id: "s-1", name: "alpha" })] });
  await app.page.getByRole("button", { name: /^alpha\b/ }).click();
  const dialog = app.page.getByRole("dialog");
  await expectSheet(app.page, dialog, /./, ["Continue"]);
  await expectNoSeriousA11yViolations(app.page, "narrow: first-run notice");
  await captureScreen(app.page, "narrow-first-run");
});

test("lg: add provider dialog is a full-screen sheet @phone", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await openFromMenu(page, "Settings");
  await page.getByRole("button", { name: "AI Provider", exact: true }).click();
  await page.getByRole("button", { name: "Add Provider", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Add Provider" });
  await expectSheet(page, dialog, "Add Provider", ["Cancel", "Add"]);
  await captureScreen(page, "narrow-add-provider");
});

test("xl: ssh_config import dialog is a full-screen sheet @phone", async ({ app }) => {
  const preview: SshConfigImportPreviewDto = {
    groups: [{ name: "config", parent: null, sourcePath: "/home/me/.ssh/config" }],
    entries: Array.from({ length: 6 }, (_, index) => ({
      index,
      name: `host-${index}`,
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
  await app.launch({ ...populated(), commands: { preview_ssh_config_import: { result: preview } } });
  const page = app.page;
  await openFromMenu(page, "Manage");
  await page.getByRole("button", { name: "Import …", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expectSheet(page, dialog, "Import ssh_config", ["Cancel"]);
  await expectNoSeriousA11yViolations(page, "narrow: ssh config import");
  await captureScreen(page, "narrow-ssh-import");
});

test("host key dialog and unlock screens pass axe @phone", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-new", name: "fresh-host", host: "fresh.example.test", port: 2222 })],
    hostKeys: { "s-new": { kind: "unknown", fingerprint: FINGERPRINT, keyType: "ssh-ed25519" } },
  });
  await app.page.getByRole("button", { name: /^fresh-host\b/ }).click();
  await expect(app.page.getByRole("dialog")).toContainText(FINGERPRINT);
  await expectNoSeriousA11yViolations(app.page, "narrow: host key");
  await captureScreen(app.page, "narrow-host-key");
});

test("unlock screen passes axe and is captured @phone", async ({ app }) => {
  await app.launch({ commands: { get_startup_state: { result: UNLOCK_STATE } } });
  await expectUsable(app.page.getByRole("button", { name: "Unlock", exact: true }));
  await expectNoSeriousA11yViolations(app.page, "narrow: unlock");
  await captureScreen(app.page, "narrow-unlock");
});

test("toasts stay inside the viewport and do not cover the composer @phone", async ({ app }) => {
  await app.launch(populated());
  const page = app.page;
  await page.getByRole("button", { name: /^db-primary\b/ }).click();
  await expect.poll(async () => (await app.calls("connect")).length).toBe(1);
  await page.evaluate(async () => {
    const path = "/src/toastBus.ts";
    const bus = await import(/* @vite-ignore */ path);
    bus.showToast({ kind: "error", message: `Could not save the settings: ${"a very long reason ".repeat(8)}` });
    bus.showToast({ kind: "info", message: "Saved", action: { label: "Undo", onClick: () => {} } });
  });
  const status = page.getByRole("status");
  await expect(page.getByText("Saved")).toBeVisible();
  await expectFullyInViewport(status);
  await expectUsable(page.getByRole("button", { name: "Undo", exact: true }));
  // Toasts sit at the top; the chat composer (bottom of the session) is never covered.
  const composer = page.locator("form textarea").first();
  await expect(composer, "chat composer is on screen").toBeVisible();
  const [t, c] = await Promise.all([status.boundingBox(), composer.boundingBox()]);
  expect(t!.y + t!.height, "toasts end above the composer").toBeLessThanOrEqual(c!.y + 0.5);
  await noHorizontalScroll(page);
  await captureScreen(page, "narrow-toasts");
});
