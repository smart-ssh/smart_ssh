// Issue #692: "Connect" in a saved server's settings.
import { expect, test } from "../support/app";
import { ACKNOWLEDGED, server } from "../support/data";


async function openForm(page: import("@playwright/test").Page) {
  await page.getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("treeitem", { name: /alpha/ }).click();
  await expect(page.getByRole("button", { name: "Delete Server" })).toBeVisible();
}

test("Connect is disabled with unsaved changes and starts the server list's flow", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-1", name: "alpha" })],
    chatSessions: {
      "s-1": [
        { sessionId: "chat-1", title: "Old chat", startedAt: "2026-01-01T00:00:00Z", endedAt: null, messageCount: 2 },
      ],
    },
    commands: { update_server: { result: null } },
  });
  const page = app.page;
  await openForm(page);
  const connect = page.locator("form").getByRole("button", { name: "Connect", exact: true });
  await expect(connect).toBeEnabled();

  await page.getByLabel("Name", { exact: true }).first().fill("alpha2");
  await expect(connect).toBeDisabled();
  await expect(connect).toHaveAttribute("title", "Save your changes first");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(connect).toBeEnabled();

  await connect.click();
  // Previous chats exist: the session picker opens, no connection yet.
  await expect(page.getByText("Old chat")).toBeVisible();
  expect(await app.calls("connect")).toEqual([]);
});

test("Connect shows the first-run notice before the first connection", async ({ app }) => {
  await app.launch({ servers: [server({ id: "s-1", name: "alpha" })] });
  const page = app.page;
  await openForm(page);
  await page.locator("form").getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Before you start" })).toBeVisible();
  expect(await app.calls("connect")).toEqual([]);
});

test("Connect switches to the open tab of the server", async ({ app }) => {
  await app.launch({ settings: { ...ACKNOWLEDGED }, servers: [server({ id: "s-1", name: "alpha" })] });
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  await expect.poll(async () => (await app.calls("connect")).length).toBe(1);
  await page.getByRole("button", { name: "Overview", exact: true }).click();
  await openForm(page);
  await page.locator("form").getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toBeVisible();
  expect(await app.calls("connect")).toHaveLength(1);
});
