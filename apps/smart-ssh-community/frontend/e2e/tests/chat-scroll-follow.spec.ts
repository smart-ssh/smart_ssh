// Issue #540: the chat follows streamed text only while the user is at the
// bottom; a scrolled-up position is kept, and the jump button returns.
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { chatInput, openSession } from "../support/session";

test("a scrolled-up position is kept while an answer streams; the button returns to the bottom", async ({
  app,
}) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "db-primary");
  const page = app.page;
  const scroller = page.locator("div.overflow-y-auto.flex-1").first();
  const metrics = () =>
    scroller.evaluate((el) => ({
      top: el.scrollTop,
      gap: el.scrollHeight - el.scrollTop - el.clientHeight,
    }));
  const stream = async (from: number, to: number) => {
    for (let i = from; i < to; i++) {
      await app.emit("chat-text-delta", { sessionId, delta: `Line ${i} of a long answer.\n\n` });
    }
  };

  await stream(0, 40);
  await expect.poll(async () => (await metrics()).gap).toBeLessThanOrEqual(16);
  await expect(page.getByTestId("chat-jump-to-latest")).toBeHidden();

  await scroller.evaluate((el) => {
    el.scrollTop = 0;
  });
  await expect(page.getByTestId("chat-jump-to-latest")).toBeVisible();
  await stream(40, 60);
  expect((await metrics()).top).toBe(0);

  await page.getByTestId("chat-jump-to-latest").focus();
  await page.keyboard.press("Enter");
  await expect(chatInput(page)).toBeFocused();
  await expect.poll(async () => (await metrics()).gap).toBeLessThanOrEqual(16);
  await expect(page.getByTestId("chat-jump-to-latest")).toBeHidden();
  await stream(60, 70);
  await expect.poll(async () => (await metrics()).gap).toBeLessThanOrEqual(16);
});

test("focused jump button hiding does not drop focus to body", async ({ app }) => {
  const { page } = app;
  await app.launch(populated());
  const sessionId = await openSession(app, "db-primary");
  for (let i = 0; i < 40; i++) {
    await app.emit("chat-text-delta", { sessionId, delta: `Line ${i} of a long answer.\n\n` });
  }
  const scroller = page.locator("div.overflow-y-auto.flex-1").first();
  await scroller.evaluate((el) => {
    el.scrollTop = 0;
  });
  const jump = page.getByTestId("chat-jump-to-latest");
  await expect(jump).toBeVisible();
  await jump.focus();
  await scroller.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await expect(jump).toBeHidden();
  const tag = await page.evaluate(() => document.activeElement?.tagName);
  expect(tag).not.toBe("BODY");
});
