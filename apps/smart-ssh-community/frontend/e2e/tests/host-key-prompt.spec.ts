// Case 4: the host key prompt of a first connection.
import { expect, test } from "../support/app";
import { ACKNOWLEDGED, server } from "../support/data";
import { expectReachableByTab, expectUsable } from "../support/layout";

const FINGERPRINT = "SHA256:Zm9vYmFyYmF6cXV4cXV1eGNvcmdlZ3JhdWx0Z2FycGx5";

test("host key prompt: shows the key, accepts nothing without a click, Reject leaves no connection", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-new", name: "fresh-host", host: "fresh.example.test", port: 2222 })],
    hostKeys: { "s-new": { kind: "unknown", fingerprint: FINGERPRINT, keyType: "ssh-ed25519" } },
  });
  const page = app.page;

  await page.getByRole("button", { name: /^fresh-host\b/ }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("fresh.example.test:2222");
  await expect(dialog).toContainText(FINGERPRINT);
  await expect(dialog).toContainText("ssh-ed25519");

  // Nothing is decided while the dialog is open.
  expect(await app.calls("confirm_host_key")).toEqual([]);

  const reject = dialog.getByRole("button", { name: "Reject", exact: true });
  const trust = dialog.getByRole("button", { name: "Trust", exact: true });
  await expectUsable(reject);
  await expectUsable(trust);
  // Both decisions are reachable by keyboard inside the dialog.
  await expectReachableByTab(page, trust, { maxPresses: 4 });
  await expectReachableByTab(page, reject, { maxPresses: 4 });

  await reject.click();
  await expect(dialog).toBeHidden();
  const decisions = await app.calls("confirm_host_key");
  expect(decisions).toHaveLength(1);
  expect(decisions[0].args.decision).toEqual({ decision: "reject" });

  // The connection is not established: an error, no session, no tab.
  await expect(page.getByText("Host key was rejected")).toBeVisible();
  const snapshot = (await page.evaluate(() => window.__e2e!.snapshot())) as { sessions: unknown[] };
  expect(snapshot.sessions).toEqual([]);
  await expect(page.locator("form textarea")).toHaveCount(0);
});

test("host key prompt: a changed key shows both fingerprints, accepts nothing without a click, Cancel leaves no connection", async ({ app }) => {
  const KNOWN = "SHA256:a25vd25rZXlrbm93bmtleWtub3dua2V5a25vd25rZXk";
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-old", name: "moved-host", host: "moved.example.test", port: 2200 })],
    hostKeys: { "s-old": { kind: "mismatch", fingerprint: FINGERPRINT, expectedFingerprint: KNOWN, keyType: "ssh-ed25519" } },
  });
  const page = app.page;

  await page.getByRole("button", { name: /^moved-host\b/ }).click();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("moved.example.test:2200");
  await expect(dialog).toContainText(KNOWN);
  await expect(dialog).toContainText(FINGERPRINT);
  await expect(dialog).toContainText("ssh-ed25519");

  expect(await app.calls("confirm_host_key")).toEqual([]);

  const cancel = dialog.getByRole("button", { name: "Cancel Connection", exact: true });
  const trustAnyway = dialog.getByRole("button", { name: "Trust Anyway", exact: true });
  await expectUsable(cancel);
  await expectUsable(trustAnyway);
  await expectReachableByTab(page, trustAnyway, { maxPresses: 4 });
  await expectReachableByTab(page, cancel, { maxPresses: 4 });

  await cancel.click();
  await expect(dialog).toBeHidden();
  const decisions = await app.calls("confirm_host_key");
  expect(decisions).toHaveLength(1);
  expect(decisions[0].args.decision).toEqual({ decision: "reject" });

  await expect(page.getByText("Host key was rejected")).toBeVisible();
  const snapshot = (await page.evaluate(() => window.__e2e!.snapshot())) as { sessions: unknown[] };
  expect(snapshot.sessions).toEqual([]);
  await expect(page.locator("form textarea")).toHaveCount(0);
});
