// Case 2: the first-run notice before the first connection.
import { expect, test } from "../support/app";
import { server } from "../support/data";

test("first-run notice: Continue needs the checkbox, and the confirmation persists", async ({ app }) => {
  await app.launch({
    servers: [server({ id: "s-1", name: "alpha" })],
    // Keep the test on the notice: the connection itself fails, so no
    // session tab covers the server list after the reload.
    commands: { connect: { error: { message: "Connection refused", code: null } } },
  });
  const page = app.page;
  const serverButton = page.getByRole("button", { name: /^alpha\b/ });

  await serverButton.click();
  const notice = page.getByRole("heading", { name: "Before you start" });
  await expect(notice).toBeVisible();

  const checkbox = page.getByRole("checkbox", { name: "I've read and understood this." });
  const continueButton = page.getByRole("button", { name: "Continue", exact: true });
  await expect(continueButton).toBeDisabled();
  // A click on the disabled button changes nothing.
  await continueButton.click({ force: true });
  await expect(notice).toBeVisible();
  expect(await app.calls("connect")).toEqual([]);

  await checkbox.check();
  await expect(continueButton).toBeEnabled();
  await checkbox.uncheck();
  await expect(continueButton).toBeDisabled();
  await checkbox.check();
  await continueButton.click();

  await expect(notice).toBeHidden();
  await expect.poll(async () => (await app.calls("connect")).length).toBe(1);
  await expect(page.getByText("Connection refused")).toBeVisible();

  // After a reload the confirmation is still stored: connecting goes
  // straight to the backend, the notice does not come back.
  await app.reload();
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  await expect.poll(async () => (await app.calls("connect")).length).toBe(1);
  await expect(page.getByText("Connection refused")).toBeVisible();
  await expect(notice).toBeHidden();
});
