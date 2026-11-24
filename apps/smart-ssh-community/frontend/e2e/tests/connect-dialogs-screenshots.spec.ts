// Review screenshots for the connect dialogs and panels (issue #1384):
// SCREENSHOT_LABEL=before|after, SCREENSHOT_SCHEME=dark for the dark theme.
import { expect, test, type AppDriver } from "../support/app";
import { emitProgress, snapshotStep } from "../support/connecting";
import { ACKNOWLEDGED, failingStepLog, populated, server } from "../support/data";
import { captureCurrent } from "../support/screenshots";

const scheme = process.env.SCREENSHOT_SCHEME === "dark" ? "dark" : "light";
const base = () => ({ settings: { ...ACKNOWLEDGED }, servers: [server({ id: "s-1", name: "alpha" })] });
const connect = (app: AppDriver) => app.page.getByRole("button", { name: /^alpha\b/ }).click();

const SCREENS: Record<string, (app: AppDriver) => Promise<void>> = {
  "hostkey-unknown": async (app) => {
    await app.launch({ ...base(), hostKeys: { "s-1": { kind: "unknown", fingerprint: "SHA256:abc", keyType: "ssh-ed25519" } } });
    await connect(app);
    await expect(app.page.getByRole("dialog")).toBeVisible();
  },
  "hostkey-mismatch": async (app) => {
    await app.launch({ ...base(), hostKeys: { "s-1": { kind: "mismatch", fingerprint: "SHA256:new", expectedFingerprint: "SHA256:old", keyType: "ssh-ed25519" } } });
    await connect(app);
    await expect(app.page.getByRole("alertdialog")).toBeVisible();
  },
  "auth-prompt": async (app) => {
    await app.launch(base());
    await app.emit("auth-prompt-needed", {
      promptId: "p1", connectId: "c1", hop: "root@alpha:22", kind: "keyboard_interactive",
      name: "Two factor", instructions: "Enter your token", prompts: [{ text: "Verification code:", echo: true }],
    });
    await expect(app.page.getByRole("dialog", { name: /root@alpha/ })).toBeVisible();
  },
  "feature-locked": async (app) => {
    await app.launch({ ...base(), commands: { connect: { error: { message: "Locked", code: null, feature_locked: { feature: "document_export", tier: "free" } } } } });
    await connect(app);
    await expect(app.page.getByRole("dialog")).toBeVisible();
  },
  "connecting-running": async (app) => {
    await app.launch({ ...base(), commands: { connect: { hold: true } } });
    await connect(app);
    await emitProgress(app, [
      snapshotStep("dnsResolution", "ok", null, null, 3),
      snapshotStep("tcpConnect", "ok", null, null, 12),
      snapshotStep("hostKeyCheck", "warning", null, null, 1),
      snapshotStep("handshake", "running", { code: "waitingForResponse" }),
    ]);
    await expect(app.page.getByTestId("connect-progress-list")).toBeVisible();
  },
  "connect-failed": async (app) => {
    await app.launch({ ...base(), commands: { connect: { hold: true } } });
    await connect(app);
    await emitProgress(app, [snapshotStep("authentication", "failed", { code: "authMethod", method: "password" }, "SSH_AUTH_FAILED")]);
    await app.release("connect", { error: { message: "Authentication failed", code: "SSH_AUTH_FAILED", connect_log: failingStepLog(2) } });
    const log = app.page.getByTestId("connect-step-log");
    if ((await log.getAttribute("open")) === null) await log.locator("summary").click();
    await expect(log.locator("[data-failed='true']")).toBeVisible();
  },
  "unusable-server": async (app) => {
    await app.launch({
      ...populated(),
      commands: { list_unusable_servers: { result: [{ id: "u-1", name: "legacy-box", host: "legacy.example.test", groupId: null, reason: "unknown_auth_method" }] } },
    });
    await app.page.getByRole("button", { name: "Manage", exact: true }).click();
    await app.page.getByRole("treeitem", { name: /legacy-box/ }).click();
    await expect(app.page.getByRole("button", { name: /Delete/ }).first()).toBeVisible();
  },
  toast: async (app) => {
    await app.launch({
      ...base(),
      hostKeys: { "s-1": { kind: "unknown", fingerprint: "SHA256:abc", keyType: "ssh-ed25519" } },
      commands: { confirm_host_key: { error: { message: "Could not save the key", code: null } } },
    });
    await connect(app);
    await app.page.getByRole("button", { name: "Trust", exact: true }).click();
    await expect(app.page.getByText("Could not save the key")).toBeVisible();
  },
};

for (const [name, open] of Object.entries(SCREENS)) {
  test(`screenshot ${name} (${scheme})`, async ({ app }) => {
    if (process.env.SCREENSHOT_LABEL === undefined) test.skip(true, "set SCREENSHOT_LABEL to capture");
    await app.page.emulateMedia({ colorScheme: scheme });
    await open(app);
    await captureCurrent(app.page, `${name}-${scheme}`);
  });
}
