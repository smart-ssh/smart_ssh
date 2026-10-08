// Case 7: the step log of a failing "Test connection".
import { expect, test } from "../support/app";
import { failingStepLog, populated } from "../support/data";

test("a failing connection test shows a collapsed step log that expands to the failing step", async ({ app }) => {
  await app.launch({
    ...populated(),
    commands: {
      test_connection: { result: { kind: "authenticationFailed", steps: failingStepLog(1) } },
    },
  });
  const page = app.page;
  await page.getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("button", { name: "🖥️ db-primary" }).click();
  await page.getByRole("button", { name: "Test Connection", exact: true }).click();
  await expect.poll(async () => (await app.calls("test_connection")).length).toBe(1);

  const log = page.getByTestId("connect-step-log");
  const failedStep = log.locator("li[data-failed=true]");
  // Collapsed: only the summary is shown.
  await expect(log).toBeVisible();
  await expect(log).not.toHaveAttribute("open");
  await expect(failedStep).toBeHidden();

  await log.getByText("Details", { exact: true }).click();
  await expect(log).toHaveAttribute("open");
  await expect(failedStep).toBeVisible();
  await expect(failedStep).toContainText("Authentication");
  await expect(failedStep).toContainText("failed (AUTH_FAILED)");
  // The steps before it are listed as successful.
  await expect(log.locator("li:not([data-failed])")).toHaveCount(4);
});
