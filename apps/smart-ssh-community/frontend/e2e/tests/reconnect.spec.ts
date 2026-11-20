// Issue #355: reconnect a lost session in the same tab (button or R key).
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { chatInput, openSession } from "../support/session";

/** Focuses the visible terminal and types `key` into it. */
async function pressInTerminal(
  app: { page: import("@playwright/test").Page },
  key: string,
) {
  await app.page.locator(".xterm-screen").locator("visible=true").click();
  await app.page.keyboard.press(key);
}

/** Issue #1366: Reconnect lives in the header's "More actions" menu. Opens the
 * menu (unless open) and returns the entry. */
async function reconnectEntry(page: import("@playwright/test").Page, name = "Reconnect") {
  const menu = page.getByRole("menu", { name: "More actions" });
  if (!(await menu.isVisible())) {
    await page.getByRole("button", { name: "More actions" }).click();
    await expect(menu).toBeVisible();
  }
  return menu.getByRole("menuitem", { name, exact: true });
}

test("a lost session is reconnected in the same tab with the button", async ({
  app,
}) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");

  await app.emit("connection-status-changed", {
    sessionId,
    status: "disconnected",
    reason: "network down",
  });
  await expect(await reconnectEntry(app.page)).toBeVisible();
  await expect(app.page.getByText("Disconnected: network down")).toBeVisible();

  const shellsBefore = (await app.calls("open_terminal")).length;

  // The reconnect runs until the backend answers: the button is disabled.
  await app.setCommand("reconnect_session", { hold: true });
  await (await reconnectEntry(app.page)).click();
  await expect(app.page.locator("header output").getByText("Reconnecting…")).toBeVisible();
  await expect(await reconnectEntry(app.page, "Reconnecting…")).toBeDisabled();
  await app.release("reconnect_session", { result: null });
  await app.emit("connection-status-changed", {
    sessionId,
    status: "connected",
    reason: null,
  });

  await expect(
    (await reconnectEntry(app.page)),
  ).toBeHidden();
  await expect(chatInput(app.page)).toBeVisible();
  const calls = await app.calls("reconnect_session");
  expect(calls.map((c) => c.args.sessionId)).toEqual([sessionId]);
  // The terminal opens a new shell for the same session.
  await expect
    .poll(async () => (await app.calls("open_terminal")).length)
    .toBe(shellsBefore + 1);
});

test("R in the dead terminal reconnects, other keys do not, R in a live terminal is input", async ({
  app,
}) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");

  await pressInTerminal(app, "r");
  await expect
    .poll(async () => (await app.calls("terminal_input")).length)
    .toBe(1);
  expect(await app.calls("reconnect_session")).toHaveLength(0);

  await app.emit("connection-status-changed", {
    sessionId,
    status: "disconnected",
    reason: null,
  });
  await expect(
    (await reconnectEntry(app.page)),
  ).toBeVisible();
  await pressInTerminal(app, "x");
  expect(await app.calls("reconnect_session")).toHaveLength(0);
  expect(await app.calls("terminal_input")).toHaveLength(1);

  await pressInTerminal(app, "R");
  await expect
    .poll(async () => (await app.calls("reconnect_session")).length)
    .toBe(1);
});

test("a failed reconnect shows the error and keeps the button", async ({
  app,
}) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");
  await app.emit("connection-status-changed", {
    sessionId,
    status: "disconnected",
    reason: null,
  });
  await app.setCommand("reconnect_session", {
    error: { message: "boom", code: "SSH_HOST_KEY_NOT_TRUSTED" },
  });

  await (await reconnectEntry(app.page))
    .click();

  await expect(app.page.getByRole("alert")).toBeVisible();
  await expect(
    (await reconnectEntry(app.page)),
  ).toBeEnabled();
});

// Issue #356: automatic reconnect — the header shows the attempts and "Stop".
test("automatic attempts show the progress, Stop ends them and the button returns", async ({
  app,
}) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");

  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "running",
    attempt: 1,
  });
  await app.emit("connection-status-changed", {
    sessionId,
    status: "disconnected",
    reason: null,
  });
  await expect(app.page.getByText("Reconnecting… (attempt 1)")).toBeVisible();
  await expect(
    (await reconnectEntry(app.page)),
  ).toBeHidden();

  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "running",
    attempt: 2,
  });
  await expect(app.page.getByText("Reconnecting… (attempt 2)")).toBeVisible();

  await app.page.getByRole("button", { name: "Stop", exact: true }).click();
  expect(
    (await app.calls("stop_auto_reconnect")).map((c) => c.args.sessionId),
  ).toEqual([sessionId]);
  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "stopped",
    attempt: 0,
  });
  await expect(
    (await reconnectEntry(app.page)),
  ).toBeVisible();
});

test("when the time runs out the header says so and the button appears", async ({
  app,
}) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");
  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "running",
    attempt: 1,
  });
  await app.emit("connection-status-changed", {
    sessionId,
    status: "disconnected",
    reason: null,
  });

  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "gave_up",
    attempt: 0,
  });

  await expect(
    app.page.getByText("Connection could not be restored."),
  ).toBeVisible();
  await expect(
    (await reconnectEntry(app.page)),
  ).toBeVisible();
});

test("a successful automatic reconnect needs no click", async ({ app }) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");
  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "running",
    attempt: 1,
  });
  const shellsBefore = (await app.calls("open_terminal")).length;
  await app.emit("connection-status-changed", {
    sessionId,
    status: "reconnecting",
    reason: null,
  });
  await app.emit("connection-status-changed", {
    sessionId,
    status: "connected",
    reason: null,
  });
  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "reconnected",
    attempt: 0,
  });
  // Issue #565: the automatic reconnect opens exactly one new shell.
  await expect
    .poll(async () => (await app.calls("open_terminal")).length)
    .toBe(shellsBefore + 1);
  await pressInTerminal(app, "x");
  await expect
    .poll(async () => (await app.calls("terminal_input")).length)
    .toBe(1);

  await expect(app.page.getByText("Reconnecting… (attempt 1)")).toBeHidden();
  await expect(
    (await reconnectEntry(app.page)),
  ).toBeHidden();
  expect(await app.calls("reconnect_session")).toHaveLength(0);
});

// Issue #565: a second loss after a reconnect is shown again.
test("a second loss after an automatic reconnect is shown again", async ({
  app,
}) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "web-1");
  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "running",
    attempt: 1,
  });
  await app.emit("connection-status-changed", {
    sessionId,
    status: "reconnecting",
    reason: null,
  });
  await app.emit("connection-status-changed", {
    sessionId,
    status: "connected",
    reason: null,
  });
  await app.emit("auto-reconnect-changed", {
    sessionId,
    phase: "reconnected",
    attempt: 0,
  });

  await app.emit("connection-status-changed", {
    sessionId,
    status: "disconnected",
    reason: "again",
  });
  await expect(
    (await reconnectEntry(app.page)),
  ).toBeVisible();
  await expect(app.page.getByText("Disconnected: again")).toBeVisible();
});
