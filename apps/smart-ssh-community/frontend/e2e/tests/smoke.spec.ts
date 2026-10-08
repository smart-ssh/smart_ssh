import { expect, test } from "../support/app";
import { EMPTY, populated } from "../support/data";

test.describe("smoke", () => {
  test("starts with an empty backend", async ({ app }) => {
    await app.launch(EMPTY);
    await expect(app.page.getByRole("heading", { name: "Smart SSH" })).toBeVisible();
    expect(app.consoleErrors).toEqual([]);
  });

  test("starts with a populated backend", async ({ app }) => {
    await app.launch(populated());
    await expect(app.page.getByText("db-primary")).toBeVisible();
    expect(app.consoleErrors).toEqual([]);
  });
});
