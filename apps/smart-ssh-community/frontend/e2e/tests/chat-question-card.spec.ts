// Issue #547 (Spec 0111): AI question card — click an option, tick several.
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { openSession } from "../support/session";

const options = [
  { label: "Alpha", description: "first option" },
  { label: "Beta", description: null },
  { label: "Gamma", description: null },
];

test("single choice: click sends the label, shows a user bubble and locks the card", async ({ app }) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "db-primary");
  await app.emit("chat-question-asked", { sessionId, questionId: "q1", question: "Which one?", options, multiSelect: false });
  const card = app.page.getByRole("group", { name: /Which one\?/ });
  await expect(card).toBeVisible();
  await expect(card.getByText("first option")).toBeVisible();
  await card.getByRole("button", { name: /Beta/ }).click();
  await expect(app.page.getByText("Beta", { exact: true })).toHaveCount(2);
  await expect(card.getByRole("button", { name: /Alpha/ })).toBeDisabled();
  const sent = await app.calls("send_chat_message");
  expect(sent.map((c) => (c.args as { text?: string }).text)).toEqual(["Beta"]);
});

test("multiple choice: ticked labels go out one per line", async ({ app }) => {
  await app.launch(populated());
  const sessionId = await openSession(app, "db-primary");
  await app.emit("chat-question-asked", { sessionId, questionId: "q2", question: "Which ones?", options, multiSelect: true });
  const card = app.page.getByRole("group", { name: /Which ones\?/ });
  const send = card.getByRole("button", { name: "Send", exact: true });
  await expect(send).toBeDisabled();
  await card.getByRole("button", { name: /Gamma/ }).click();
  await card.getByRole("button", { name: /Alpha/ }).click();
  await send.click();
  const sent = await app.calls("send_chat_message");
  expect(sent.map((c) => (c.args as { text?: string }).text)).toEqual(["Alpha\nGamma"]);
  await expect(card.getByRole("button", { name: /Gamma/ })).toBeDisabled();
});
