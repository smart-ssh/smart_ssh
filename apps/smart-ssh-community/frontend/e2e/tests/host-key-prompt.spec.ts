// Case 4: the host key prompt of a first connection.
import { expect, test } from "../support/app";
import { ACKNOWLEDGED, server } from "../support/data";
import { expectReachableByTab, expectUsable } from "../support/layout";

const FINGERPRINT = "SHA256:Zm9vYmFyYmF6cXV4cXV1eGNvcmdlZ3JhdWx0Z2FycGx5";

test("host key prompt: shows the key, accepts nothing without a click, Reject leaves no connection", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-new", name: "fresh-host", host: "fresh.example.test", port: 2222 })],
    hostKeys: { "s-new": { kind: "unknown", fingerprint: FINGERPRINT } },
  });
  const page = app.page;

  await page.getByRole("button", { name: /^fresh-host\b/ }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("fresh.example.test:2222");
  await expect(dialog).toContainText(FINGERPRINT);

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
