// Issue #581, Spec 0113 §5: dialogs, host key prompt and start-up screens at
// phone width. Tagged `@phone`: runs in the phone projects only.
import { expect, test } from "../support/app";
import { ACKNOWLEDGED, server } from "../support/data";
import { expectFullyInViewport, expectUsable } from "../support/layout";

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
