// Issue #1385: screenshots of the move-to dialog (reachable at phone width).
import { expect, test } from "../support/app";
import { ACKNOWLEDGED, populated } from "../support/data";
import { captureScreen } from "../support/screenshots";

test("move-to dialog is captured", async ({ app }) => {
  await app.launch({ ...populated(), settings: { ...ACKNOWLEDGED } });
  const page = app.page;
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Open navigation menu", exact: true }).click();
  await page.getByRole("dialog", { name: "Navigation menu" }).getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("button", { name: "Move “standalone” to…", exact: true }).click();
  await expect(page.getByRole("dialog").getByText("current")).toHaveCount(1);
  await captureScreen(page, "move-to-dialog");
});
