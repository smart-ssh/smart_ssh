// Issue #1367: on the wide layout the Connect tree rows are single-line (30-32 px).
import { expect, test } from "../support/app";
import { populated, server } from "../support/data";
import { captureScreen } from "../support/screenshots";
import { openSession } from "../support/session";

test.describe("wide Connect tree", () => {
  test("server rows are single-line, at most 36 px high", async ({ app }, testInfo) => {
    test.skip(testInfo.project.name.includes("phone"), "wide layout only");
    await app.launch(populated());
    const row = app.page.getByRole("button", { name: /^web-1\b/ });
    await expect(row).toBeVisible();
    const h = (await row.boundingBox())!.height;
    expect(h).toBeGreaterThanOrEqual(30);
    expect(h).toBeLessThanOrEqual(36);
  });
});

// Review screenshots (SCREENSHOT_LABEL=before|after, SCREENSHOT_SCHEME=dark): the tree with the
// Localhost row and a status square of an open session, at desktop and 390 px.
test.describe("Connect tree screenshots", () => {
  test("tree with Localhost row and status square", async ({ app }) => {
    const base = populated();
    await app.launch({
      ...base,
      servers: [server({ id: "s-local", name: "Localhost", host: "localhost", isLocal: true, port: 0 }), ...base.servers!],
    });
    if (process.env.SCREENSHOT_SCHEME === "dark") await app.page.emulateMedia({ colorScheme: "dark" });
    await openSession(app, "web-1");
    await app.page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(app.page.getByRole("button", { name: /^Localhost\b/ })).toBeVisible();
    await captureScreen(app.page, "connect-tree");
  });
});
