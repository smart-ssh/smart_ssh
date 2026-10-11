// Issue #355: reconnect a lost session in the same tab (button or R key).
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { chatInput, openSession } from "../support/session";

const terminalKeys = (app: { page: import("@playwright/test").Page }) =>
  app.page.locator(".xterm-helper-textarea").locator("visible=true");

test("a lost session is reconnected in the same tab with the button", async ({ app }) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");

  await app.emit("connection-status-changed", {
    sessionId,
    status: "disconnected",
    reason: "network down",
  });
  const button = app.page.getByRole("button", { name: "Reconnect", exact: true });
  await expect(button).toBeVisible();
  await expect(app.page.getByText("Disconnected: network down")).toBeVisible();

  // The reconnect runs until the backend answers: the button is disabled.
  await app.setCommand("reconnect_session", { hold: true });
  await button.click();
  await expect(app.page.getByRole("button", { name: "Reconnecting…" })).toBeDisabled();
  await app.release("reconnect_session", { result: null });
  await app.emit("connection-status-changed", { sessionId, status: "connected", reason: null });

  await expect(app.page.getByRole("button", { name: "Reconnect", exact: true })).toBeHidden();
  await expect(chatInput(app.page)).toBeVisible();
  const calls = await app.calls("reconnect_session");
  expect(calls.map((c) => c.args.sessionId)).toEqual([sessionId]);
  // The terminal opens a new shell for the same session.
  await expect.poll(async () => (await app.calls("open_terminal")).length).toBe(2);
});

test("R in the dead terminal reconnects, other keys do not, R in a live terminal is input", async ({
  app,
}) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");

  await terminalKeys(app).press("r");
  await expect.poll(async () => (await app.calls("terminal_input")).length).toBe(1);
  expect(await app.calls("reconnect_session")).toHaveLength(0);

  await app.emit("connection-status-changed", { sessionId, status: "disconnected", reason: null });
  await expect(app.page.getByRole("button", { name: "Reconnect", exact: true })).toBeVisible();
  await terminalKeys(app).press("x");
  expect(await app.calls("reconnect_session")).toHaveLength(0);
  expect(await app.calls("terminal_input")).toHaveLength(1);

  await terminalKeys(app).press("R");
  await expect.poll(async () => (await app.calls("reconnect_session")).length).toBe(1);
});

test("a failed reconnect shows the error and keeps the button", async ({ app }) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");
  await app.emit("connection-status-changed", { sessionId, status: "disconnected", reason: null });
  await app.setCommand("reconnect_session", {
    error: { message: "boom", code: "SSH_HOST_KEY_NOT_TRUSTED" },
  });

  await app.page.getByRole("button", { name: "Reconnect", exact: true }).click();

  await expect(app.page.getByRole("alert")).toBeVisible();
  await expect(app.page.getByRole("button", { name: "Reconnect", exact: true })).toBeEnabled();
});
