// Issue #1367: on the wide layout the Connect tree rows are single-line (30-32 px).
import { expect, test } from "../support/app";
import { populated } from "../support/data";

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
