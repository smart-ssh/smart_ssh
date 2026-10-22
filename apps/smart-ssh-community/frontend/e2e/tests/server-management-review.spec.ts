// UI review of the server tree and server management (issue #382): axe scan,
// Tab order, Escape + focus return and clipping for every screen in scope.
import type { SshConfigExportResultDto, SshConfigImportPreviewDto } from "../../src/types";
import { expect, test, type AppDriver } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { ACKNOWLEDGED, failingStepLog, group, populated, server } from "../support/data";
import { assertVisualOrder, walkTabOrder } from "../support/keyboard";
import { expectNoClippedText } from "../support/layout";
import { captureScreen } from "../support/screenshots";

const LONG = "very-long-server-name-".repeat(6);

function importPreview(): SshConfigImportPreviewDto {
  return {
    groups: [{ name: "config", parent: null, sourcePath: "/home/me/.ssh/config" }],
    entries: Array.from({ length: 6 }, (_, index) => ({
      index,
      name: index === 0 ? LONG : `host-${index}`,
      group: 0,
      host: { value: `host-${index}.example.test`, origin: null },
      port: { value: 22, origin: null },
      username: { value: "deploy", origin: null },
      tags: [],
      identityFile: null,
      jumpHost: null,
      conflict: null,
    })),
    files: [{ path: "/home/me/.ssh/config", depth: 0, status: "read" }],
    skipped: [],
    identityFilePaths: [],
  };
}

const EXPORT_RESULT: SshConfigExportResultDto = {
  path: "/home/me/.ssh/smart-ssh-config",
  exported: [],
  includeHint: "Include /home/me/.ssh/smart-ssh-config",
};

function longContent() {
  const base = populated();
  return {
    ...base,
    groups: [...(base.groups ?? []), group({ id: "g-long", name: `Group ${LONG}` })],
    servers: [
      ...(base.servers ?? []),
      server({ id: "s-long", name: LONG, host: `${LONG}.example.test`, groupId: "g-long", tags: ["a-very-long-tag-name-".repeat(3)] }),
    ],
  };
}

interface Screen {
  open: (app: AppDriver) => Promise<void>;
  /** Tab presses to walk (the tree is one stop, so a few suffice). */
  tabs?: number;
}

const manage = async (app: AppDriver) => {
  await app.page.getByRole("button", { name: "Manage", exact: true }).click();
};

const SCREENS: Record<string, Screen> = {
  "server-list-main": {
    open: async (app) => {
      await app.launch(longContent());
      await expect(app.page.getByText("db-primary")).toBeVisible();
    },
  },
  "server-list-empty": {
    open: async (app) => {
      await app.launch({ settings: { ...ACKNOWLEDGED } });
      await expect(app.page.getByRole("button", { name: /first server/i })).toBeVisible();
    },
  },
  "management-empty": {
    open: async (app) => {
      await app.launch(longContent());
      await manage(app);
      await expect(app.page.getByText("Select a group or server on the left")).toBeVisible();
    },
  },
  "management-server-form": {
    open: async (app) => {
      await app.launch(longContent());
      await manage(app);
      await app.page.getByRole("treeitem", { name: `🖥️ ${LONG}` }).click();
      await expect(app.page.getByRole("tab", { name: "General", exact: true })).toHaveAttribute("aria-selected", "true");
      await expect(app.page.getByRole("button", { name: "Delete Server" })).toBeVisible();
    },
    tabs: 40,
  },
  "management-server-form-connection": {
    open: async (app) => {
      await app.launch(longContent());
      await manage(app);
      await app.page.getByRole("treeitem", { name: `🖥️ ${LONG}` }).click();
      await app.page.getByRole("tab", { name: "Connection", exact: true }).click();
      await expect(app.page.getByRole("button", { name: "Test Connection", exact: true })).toBeVisible();
    },
    tabs: 40,
  },
  "management-server-form-security": {
    open: async (app) => {
      await app.launch(longContent());
      await manage(app);
      await app.page.getByRole("treeitem", { name: `🖥️ ${LONG}` }).click();
      await app.page.getByRole("tab", { name: "Security", exact: true }).click();
      await expect(app.page.getByRole("radio").first()).toBeVisible();
    },
    tabs: 30,
  },
  "management-server-form-notes": {
    open: async (app) => {
      await app.launch(longContent());
      await manage(app);
      await app.page.getByRole("treeitem", { name: `🖥️ ${LONG}` }).click();
      await app.page.getByRole("tab", { name: "Notes", exact: true }).click();
      await expect(app.page.getByRole("button", { name: "Context Preview" }).first()).toBeVisible();
    },
    tabs: 30,
  },
  "management-new-server": {
    open: async (app) => {
      await app.launch(longContent());
      await manage(app);
      await app.page.getByRole("button", { name: "+ Server", exact: true }).click();
      await expect(app.page.getByRole("tab")).toHaveCount(3);
      await app.page.getByRole("tab", { name: "Connection", exact: true }).click();
      await expect(app.page.getByRole("button", { name: "Test Connection", exact: true })).toBeVisible();
    },
    tabs: 40,
  },
  "management-group-form": {
    open: async (app) => {
      await app.launch(longContent());
      await manage(app);
      await app.page.getByRole("treeitem", { name: "📁 Production" }).click();
      await expect(app.page.getByRole("heading").first()).toBeVisible();
    },
    tabs: 30,
  },
  "management-new-group": {
    open: async (app) => {
      await app.launch(longContent());
      await manage(app);
      await app.page.getByRole("button", { name: "+ Group", exact: true }).click();
    },
    tabs: 30,
  },
  "management-unusable-server": {
    open: async (app) => {
      await app.launch({
        ...longContent(),
        commands: {
          list_unusable_servers: {
            result: [{ id: "u-1", name: "legacy-box", host: "legacy.example.test", groupId: null, reason: "unknown_auth_method" }],
          },
        },
      });
      await manage(app);
      await app.page.getByRole("treeitem", { name: /legacy-box/ }).click();
      await expect(app.page.getByRole("button", { name: /Delete/ }).first()).toBeVisible();
    },
  },
  "connect-step-log": {
    open: async (app) => {
      await app.launch({
        ...populated(),
        commands: { test_connection: { result: { kind: "authenticationFailed", steps: failingStepLog(2) } } },
      });
      await manage(app);
      await app.page.getByRole("treeitem", { name: "🖥️ db-primary" }).click();
      await app.page.getByRole("tab", { name: "Connection", exact: true }).click();
      await app.page.getByRole("button", { name: "Test Connection", exact: true }).click();
      await app.page.getByText("Details", { exact: true }).click();
      await expect(app.page.getByTestId("connect-step-log")).toHaveAttribute("open");
    },
    tabs: 40,
  },
};

interface DialogScreen {
  open: (app: AppDriver) => Promise<void>;
  trigger: (app: AppDriver) => ReturnType<AppDriver["page"]["getByRole"]>;
  /** Locator that is visible while the dialog is open. */
  marker: (app: AppDriver) => ReturnType<AppDriver["page"]["getByRole"]>;
}

const DIALOGS: Record<string, DialogScreen> = {
  "ssh-config-import": {
    open: async (app) => {
      await app.launch({ ...longContent(), commands: { preview_ssh_config_import: { result: importPreview() } } });
      await manage(app);
    },
    trigger: (app) => app.page.getByRole("button", { name: "Import …", exact: true }),
    marker: (app) => app.page.getByRole("heading", { name: "Import ssh_config" }),
  },
  "ssh-config-export": {
    open: async (app) => {
      await app.launch({ ...longContent(), commands: { export_ssh_config: { result: EXPORT_RESULT } } });
      await manage(app);
    },
    trigger: (app) => app.page.getByRole("button", { name: "Export …", exact: true }),
    marker: (app) => app.page.getByRole("heading", { name: /Export/ }),
  },
  "chat-session-picker": {
    open: async (app) => {
      await app.launch({
        ...longContent(),
        commands: {
          list_chat_sessions: {
            result: [
              { sessionId: "c1", title: LONG, startedAt: "2026-01-02T10:00:00Z", endedAt: null, messageCount: 12 },
              { sessionId: "c2", title: null, startedAt: "2026-01-01T10:00:00Z", endedAt: null, messageCount: 3 },
            ],
          },
        },
      });
      await expect(app.page.getByText("db-primary")).toBeVisible();
    },
    trigger: (app) => app.page.getByRole("button", { name: /db-primary/ }).first(),
    marker: (app) => app.page.getByRole("button", { name: "New conversation" }),
  },
};

test.describe("server management review", () => {
  for (const [name, screen] of Object.entries(SCREENS)) {
    test(`${name}: axe, Tab order, no clipped text`, async ({ app }) => {
      test.setTimeout(90_000);
      await screen.open(app);
      await captureScreen(app.page, name);
      await expectNoSeriousA11yViolations(app.page, name);
      await expectNoClippedText(app.page.locator("body"));
      await app.page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      assertVisualOrder(await walkTabOrder(app.page, screen.tabs ?? 20));
    });
  }

  for (const [name, dialog] of Object.entries(DIALOGS)) {
    test(`${name}: axe, Tab order, Escape returns focus to the trigger`, async ({ app }) => {
      test.setTimeout(90_000);
      await dialog.open(app);
      const trigger = dialog.trigger(app);
      await trigger.focus();
      await trigger.press("Enter");
      await expect(dialog.marker(app)).toBeVisible();
      await captureScreen(app.page, name);
      await expectNoSeriousA11yViolations(app.page, name);
      await expectNoClippedText(app.page.locator("body"));
      assertVisualOrder(await walkTabOrder(app.page, 12));
      await app.page.keyboard.press("Escape");
      await expect(dialog.marker(app)).toBeHidden();
      await expect(trigger).toBeFocused();
    });
  }
});
