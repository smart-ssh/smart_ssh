// Issue #548: a connecting tab with live steps opens right away.
import { expect, test } from "../support/app";
import { emitProgress, lastAttemptId, snapshotStep } from "../support/connecting";
import { ACKNOWLEDGED, failingStepLog, server } from "../support/data";
import { expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";
import { expectNoSeriousA11yViolations } from "../support/axe";

const launchHeld = (app: Parameters<Parameters<typeof test>[2]>[0]["app"]) =>
  app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-1", name: "alpha" }), server({ id: "s-2", name: "beta" })],
    commands: { connect: { hold: true } },
  });

test("opens a focused connecting tab before connect resolves and lists the steps live", async ({ app }) => {
  await launchHeld(app);
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();

  await expect(page.getByRole("heading", { name: "Connecting to alpha …" })).toBeVisible();
  await expect(page.getByRole("button", { name: /^Connecting to alpha/ })).toHaveAttribute("aria-current", "page");

  await emitProgress(app, [
    snapshotStep("dnsResolution", "ok", { code: "contacting", target: "h.example.test:22" }),
    snapshotStep("tcpConnect", "ok", { code: "contacting", target: "10.0.0.1:22" }),
    snapshotStep("hostKeyCheck", "running", { code: "waitingForDecision" }),
  ]);
  const rows = page.getByTestId("connect-progress-list").getByRole("listitem");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toContainText("DNS resolution");
  await expect(rows.nth(0)).toContainText("h.example.test:22");
  await expect(rows.nth(2)).toContainText("waiting for your decision");
  await expectNoSeriousA11yViolations(page, "connecting tab");
});

test("turns into the session tab in place on success", async ({ app }) => {
  await launchHeld(app);
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  await expect(page.getByRole("heading", { name: "Connecting to alpha …" })).toBeVisible();
  await app.release("connect", { result: "session-held" });

  await expect(page.getByRole("heading", { name: "Connecting to alpha …" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toBeVisible();
  const tabs = page.getByRole("toolbar").getByRole("button", { name: /^alpha$/ });
  await expect(tabs).toHaveCount(1);
  await expect(tabs).toHaveAttribute("aria-current", "page");
  await expect(page.getByText(/Connection to alpha failed/)).toHaveCount(0);
});

test("a failed connect keeps the tab with the failed step, error, Retry and Close; nothing above the list", async ({ app }) => {
  await launchHeld(app);
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  await emitProgress(app, [
    snapshotStep("tcpConnect", "ok"),
    snapshotStep("authentication", "failed", { code: "authMethod", method: "password" }, "SSH_AUTH_FAILED"),
  ]);
  await app.release("connect", { error: { message: "Authentication failed", code: "SSH_AUTH_FAILED" } });

  await expect(page.getByRole("heading", { name: "Connection to alpha failed" })).toBeVisible();
  await expect(page.locator("[data-failed='true']")).toHaveCount(1);
  await expect(page.getByRole("alert")).not.toHaveText("");
  const retry = page.getByRole("button", { name: "Retry", exact: true });
  await expect(retry).toBeFocused();
  await expect(page.getByRole("button", { name: "Close", exact: true })).toBeVisible();
  await expectNoSeriousA11yViolations(page, "failed connect tab");

  // Retry: new attempt in the same tab, step list reset.
  const firstAttempt = await lastAttemptId(app);
  await retry.click();
  await expect.poll(async () => (await app.calls("connect")).length).toBe(2);
  expect(await lastAttemptId(app)).not.toBe(firstAttempt);
  await expect(page.getByRole("heading", { name: "Connecting to alpha …" })).toBeVisible();
  await expect(page.getByTestId("connect-progress-list")).toHaveCount(0);
  await expect(page.getByRole("button", { name: /^(Connecting to|Connection to) alpha/ })).toHaveCount(1);

  // Overview shows no connect error above the list.
  await page.getByRole("button", { name: "Overview", exact: true }).click();
  await expect(page.getByText("Authentication failed")).toHaveCount(0);
});

test("closing the tab or Cancel cancels the attempt without an error", async ({ app }) => {
  await launchHeld(app);
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  const attemptId = await lastAttemptId(app);
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect.poll(async () => (await app.calls("cancel_connect")).length).toBe(1);
  expect((await app.calls("cancel_connect"))[0].args.attemptId).toBe(attemptId);
  await app.release("connect", { error: { message: "cancelled", code: "SSH_CONNECT_CANCELLED" } });
  await expect(page.getByRole("heading", { name: /Connect/ })).toHaveCount(0);
  await expect(page.getByText("cancelled")).toHaveCount(0);

  await page.getByRole("button", { name: /^beta\b/ }).click();
  await page.getByRole("button", { name: /^Close tab "Connecting to beta/ }).click();
  await expect.poll(async () => (await app.calls("cancel_connect")).length).toBe(2);
});

test("a second click on a connecting server focuses its tab without a second connect", async ({ app }) => {
  await launchHeld(app);
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  await page.getByRole("button", { name: "Overview", exact: true }).click();
  await expect(page.locator("span", { hasText: "Connecting…" })).toBeVisible();
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  await expect(page.getByRole("heading", { name: "Connecting to alpha …" })).toBeVisible();
  expect(await app.calls("connect")).toHaveLength(1);
});

test("host key prompt during connect: dialog as before, step continues in the same tab after Trust", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-1", name: "alpha" })],
    hostKeys: { "s-1": { kind: "unknown", fingerprint: "SHA256:abc", keyType: "ssh-ed25519" } },
  });
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await emitProgress(app, [snapshotStep("hostKeyCheck", "running", { code: "waitingForDecision" })]);
  await expect(page.getByTestId("connect-progress-list")).toContainText("waiting for your decision");
  await dialog.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toBeVisible();
  await expect(page.getByRole("toolbar").getByRole("button", { name: /^alpha$/ })).toHaveCount(1);
});

// Issue #568: resuming a saved chat session uses the same connecting tab.
test("resuming a saved chat session shows the connecting tab, then the session tab", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-1", name: "alpha" })],
    chatSessions: {
      "s-1": [
        { sessionId: "chat-1", title: "Old chat", startedAt: "2026-01-01T00:00:00Z", endedAt: null, messageCount: 2 },
      ],
    },
    commands: { resume_chat_session: { hold: true } },
  });
  const page = app.page;
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  await page.getByText("Old chat").click();

  await expect(page.getByRole("heading", { name: "Connecting to alpha …" })).toBeVisible();
  const calls = await app.calls("resume_chat_session");
  expect(calls).toHaveLength(1);
  expect(JSON.stringify(calls[0])).toContain("chat-1");

  await app.release("resume_chat_session", { result: "session-resumed" });
  await expect(page.getByRole("heading", { name: "Connecting to alpha …" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toBeVisible();
  await expect(page.getByRole("toolbar").getByRole("button", { name: /^alpha$/ })).toHaveCount(1);
});

// Issue #582, Spec 0113 §5a: the connecting tab at phone width.
test("connecting tab at phone width: progress, wrapped step log and Retry/Close visible @phone", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-1", name: "alpha" })],
    commands: { connect: { hold: true } },
  });
  const page = app.page;
  const noHScroll = async () =>
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.getByRole("button", { name: /^alpha\b/ }).click();
  await emitProgress(app, [
    snapshotStep("dnsResolution", "ok", { code: "contacting", target: "a-very-long-hostname.internal.example.test:22" }),
    snapshotStep("tcpConnect", "running", { code: "contacting", target: "10.0.0.1:22" }),
  ]);
  await expect(page.getByTestId("connect-progress-list").getByRole("listitem")).toHaveCount(2);
  await expectUsable(page.getByRole("button", { name: "Cancel", exact: true }));
  await noHScroll();
  await captureScreen(page, "connecting-tab-narrow-progress");

  await app.release("connect", {
    error: { message: "Authentication failed", code: "SSH_AUTH_FAILED", connect_log: failingStepLog(2) },
  });
  await expect(page.getByRole("heading", { name: "Connection to alpha failed" })).toBeVisible();
  await page.getByTestId("connect-step-log").locator("summary").click();
  await expect(page.locator("[data-failed='true']")).toBeVisible();
  await expectUsable(page.getByRole("button", { name: "Retry", exact: true }));
  await expectUsable(page.getByRole("button", { name: "Close", exact: true }));
  await noHScroll();
  await expectNoSeriousA11yViolations(page, "failed connect tab, phone");
  await captureScreen(page, "connecting-tab-narrow-failed");
});
