// UI review of the start-up, unlock, first-run and host key screens (issue
// #387): Tab order follows the visual order with a visible focus indicator,
// Escape and focus return behave per docs/ui-guidelines.md, and long content
// keeps title and buttons usable.
import { expect, test } from "../support/app";
import { ACKNOWLEDGED, server } from "../support/data";
import { captureScreen } from "../support/screenshots";
import { expectOnlyContentScrolls, expectUsable, scrollContainerWithin, walkTabOrder } from "../support/layout";

const UNLOCK_STATE = {
  screen: "unlock",
  mode: "password",
  offersStartOver: true,
  failedUnlockAttempts: 3,
  language: "en",
};

test("unlock screen: Tab follows the visual order and every stop shows focus", async ({ app }) => {
  await app.launch({ commands: { get_startup_state: { result: UNLOCK_STATE } } });
  const page = app.page;
  const password = page.getByLabel("Master password");
  await expect(password).toBeFocused();
  await password.fill("correct horse battery");

  const stops = await walkTabOrder(page, 3);
  expect(stops.map((s) => s.name)).toEqual(["Unlock", "Quit", "Start over"]);
  expect(stops.filter((s) => !s.indicator).map((s) => s.name)).toEqual([]);
  await expectUsable(page.getByRole("button", { name: "Start over", exact: true }));
});

test("startup question: Escape cancels a confirmation, a long message scrolls, buttons stay usable", async ({ app }) => {
  await app.launch({ commands: { get_startup_state: { result: UNLOCK_STATE } } });
  const page = app.page;
  const message = Array.from({ length: 60 }, (_, i) => `Line ${i}: details about the data file`).join("\n");
  await app.emit("startup:prompt", { kind: "confirmStartOver", title: "Start over?", message });

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  const title = dialog.getByRole("heading", { name: "Start over?" });
  const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
  const confirm = dialog.getByRole("button", { name: "Yes, continue", exact: true });
  await expectUsable(title);
  await expectUsable(cancel);
  await expectUsable(confirm);
  // The safest button holds the initial focus.
  await expect(cancel).toBeFocused();
  await expectOnlyContentScrolls(await scrollContainerWithin(dialog, dialog.getByText("Line 59")), [title, cancel, confirm]);

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  const answers = await app.calls("answer_startup_prompt");
  expect(answers).toHaveLength(1);
  expect(answers[0].args.answer).toBe("cancel");
});

test("startup notice: Escape dismisses it without sending an answer", async ({ app }) => {
  await app.launch({ commands: { get_startup_state: { result: UNLOCK_STATE } } });
  const page = app.page;
  await app.emit("startup:prompt", { kind: "notice", title: "Renamed", message: "The file was renamed." });
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "OK", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(await app.calls("answer_startup_prompt")).toEqual([]);
});

test("new master password: fields in Tab order, Enter submits only when usable", async ({ app }) => {
  await app.launch({ commands: { get_startup_state: { result: UNLOCK_STATE } } });
  const page = app.page;
  await app.emit("startup:prompt", { kind: "newMasterPassword", title: "Set a master password", message: "Choose one." });
  const dialog = page.getByRole("dialog");
  const first = dialog.getByLabel("New master password");
  await expect(first).toBeFocused();
  await first.fill("a-long-enough-password");
  const stops = await walkTabOrder(page, 4);
  expect(stops.map((s) => s.name.slice(0, 12))).toEqual([
    "Repeat maste",
    "I understand",
    "Cancel",
    // "Yes, continue" is disabled and skipped: the order wraps to the first field.
    "New master p",
  ]);
  expect(stops.filter((s) => !s.indicator).map((s) => s.name)).toEqual([]);
  // "Yes, continue" is disabled until repeated and acknowledged: Enter in a field sends nothing.
  await first.press("Enter");
  expect(await app.calls("answer_startup_prompt")).toEqual([]);
});

test("first-run notice: Tab order, no focus loss, Escape does not skip the confirmation", async ({ app }) => {
  await app.launch({
    servers: [server({ id: "s-1", name: "alpha" })],
    commands: { connect: { error: { message: "Connection refused", code: null } } },
  });
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  const dialog = page.getByRole("dialog");
  const checkbox = dialog.getByRole("checkbox");
  await expect(checkbox).toBeFocused();
  await checkbox.check();
  const stops = await walkTabOrder(page, 1);
  expect(stops.map((s) => s.name)).toEqual(["Continue"]);
  expect(stops[0].indicator).toBe(true);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();
  expect(await app.calls("connect")).toEqual([]);
});

test("host key dialog: Escape rejects, Tab cycles Trust and Reject inside the dialog", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-new", name: "fresh-host", host: "fresh.example.test" })],
    hostKeys: { "s-new": { kind: "unknown", fingerprint: "SHA256:abc", keyType: "ssh-ed25519" } },
  });
  const page = app.page;
  const trigger = page.getByRole("button", { name: /^fresh-host\b/ });
  await trigger.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("button", { name: "Reject", exact: true })).toBeFocused();
  const stops = await walkTabOrder(page, 2);
  expect(stops.map((s) => s.name)).toEqual(["Trust", "Reject"]);
  expect(stops.filter((s) => !s.indicator).map((s) => s.name)).toEqual([]);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  const decisions = await app.calls("confirm_host_key");
  expect(decisions[0].args.decision).toEqual({ decision: "reject" });
  // Focus return to the server button is not asserted: the list disables its
  // buttons while a connection is pending, so the browser cannot focus it
  // (reported as a follow-up for the server list).
});

test("host key dialog: long host name and both fingerprints stay fully visible, buttons usable", async ({ app }) => {
  const longHost = `${"very-long-subdomain-".repeat(6)}example.test`;
  const known = `SHA256:${"K".repeat(43)}`;
  const offered = `SHA256:${"O".repeat(43)}`;
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-new", name: "fresh-host", host: longHost })],
    hostKeys: { "s-new": { kind: "mismatch", fingerprint: offered, expectedFingerprint: known, keyType: "ssh-ed25519" } },
  });
  const page = app.page;
  await page.getByRole("button", { name: /^fresh-host\b/ }).click();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toContainText(known);
  await expect(dialog).toContainText(offered);
  await expectUsable(dialog.getByText(known));
  await expectUsable(dialog.getByText(offered));
  await expectUsable(dialog.getByRole("button", { name: "Cancel Connection", exact: true }));
  await expectUsable(dialog.getByRole("button", { name: "Trust Anyway", exact: true }));
});

// Before/after images for the review PR: `SCREENSHOT_LABEL=before|after npm run test:e2e -- startup-dialogs`.
test("screenshots of the scoped screens", async ({ app }) => {
  test.skip(!process.env.SCREENSHOT_LABEL, "only when SCREENSHOT_LABEL is set");
  await app.launch({ commands: { get_startup_state: { result: UNLOCK_STATE } } });
  await captureScreen(app.page, "startup-unlock");
  await app.emit("startup:prompt", {
    kind: "newMasterPassword",
    title: "Set a master password",
    message: "Choose one.",
  });
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "startup-new-master-password");
});

test("screenshots of the startup question", async ({ app }) => {
  test.skip(!process.env.SCREENSHOT_LABEL, "only when SCREENSHOT_LABEL is set");
  await app.launch({ commands: { get_startup_state: { result: UNLOCK_STATE } } });
  await app.emit("startup:prompt", { kind: "confirmStartOver", title: "Start over?", message: "This deletes the data file." });
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "startup-question");
});

test("screenshots of the first-run notice", async ({ app }) => {
  test.skip(!process.env.SCREENSHOT_LABEL, "only when SCREENSHOT_LABEL is set");
  await app.launch({ servers: [server({ id: "s-1", name: "alpha" })] });
  await app.page.getByRole("button", { name: /^alpha\b/ }).click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "first-run-notice");
});

test("screenshots of the host key dialog", async ({ app }) => {
  test.skip(!process.env.SCREENSHOT_LABEL, "only when SCREENSHOT_LABEL is set");
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-new", name: "fresh-host", host: "fresh.example.test" })],
    hostKeys: { "s-new": { kind: "unknown", fingerprint: "SHA256:abc", keyType: "ssh-ed25519" } },
  });
  await app.page.getByRole("button", { name: /^fresh-host\b/ }).click();
  await expect(app.page.getByRole("dialog")).toBeVisible();
  await captureScreen(app.page, "host-key-unknown");
});
