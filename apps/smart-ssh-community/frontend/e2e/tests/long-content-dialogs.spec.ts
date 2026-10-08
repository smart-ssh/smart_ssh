// Case 5: dialogs with content of any length. Only the content area
// scrolls; the header and the buttons stay inside the window, uncovered
// and clickable — in the small window too.
import type {
  NoteUpdateSuggestedEvent,
  SshConfigImportPreviewDto,
  SshConfigPreviewEntryDto,
} from "../../src/types";
import { THIRD_PARTY_NOTICES_MARKER } from "../../src/thirdPartyNoticesMarker";
import { expect, test } from "../support/app";
import { failingStepLog, populated, server, ACKNOWLEDGED } from "../support/data";
import { expectOnlyContentScrolls, expectUsable, scrollContainerWithin } from "../support/layout";

test("third-party licenses: the license text scrolls, title and Close stay usable", async ({ app }) => {
  const notices = [
    THIRD_PARTY_NOTICES_MARKER,
    ...Array.from({ length: 1500 }, (_, i) => `package-${i} — MIT License — Copyright (c) ${i} Example`),
  ].join("\n");
  await app.page.route("**/third-party-notices.txt", (route) =>
    route.fulfill({ status: 200, contentType: "text/plain", body: notices }),
  );
  await app.launch(populated());
  const page = app.page;

  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("button", { name: "About", exact: true }).click();
  await page.getByRole("button", { name: "Third-party licenses", exact: true }).click();

  const title = page.getByRole("heading", { name: "Third-party licenses" });
  const dialog = page.locator("div.flex-col", { has: title }).last();
  const text = dialog.locator("pre");
  await expect(text).toContainText("package-1499");
  const close = dialog.getByRole("button", { name: "Close", exact: true });

  await expectUsable(title);
  await expectUsable(close);
  await expectOnlyContentScrolls(await scrollContainerWithin(dialog, text), [title, close]);
  await expectUsable(close);

  await close.click();
  await expect(title).toBeHidden();
});

test("note diff preview: the diff scrolls, title and decision buttons stay usable", async ({ app }) => {
  await app.launch(populated());
  const previous = Array.from({ length: 120 }, (_, i) => `line ${i}: unchanged`).join("\n");
  const next = Array.from({ length: 120 }, (_, i) => `line ${i}: ${i % 2 ? "changed" : "unchanged"}`).join("\n");
  const event: NoteUpdateSuggestedEvent = {
    sessionId: "session-x",
    actionId: "note-1",
    action: { ProposeNoteUpdate: { target: "CurrentServer", new_content: next } },
    previousNoteContent: previous,
    targetName: "db-primary",
    summaryIncomplete: false,
  };
  await app.emit("note-update-suggested", event);
  const page = app.page;
  await page.getByRole("button", { name: "Show", exact: true }).click();

  const title = page.getByText('Note suggestion for server "db-primary"');
  const card = page.locator("div.rounded-lg", { has: title });
  const reject = card.getByRole("button", { name: "Reject", exact: true });
  const apply = card.getByRole("button", { name: "Apply", exact: true });
  const diffLine = card.getByText("line 119: changed");

  await expectUsable(title);
  await expectUsable(reject);
  await expectUsable(apply);
  await expectOnlyContentScrolls(await scrollContainerWithin(card, diffLine), [title, reject, apply]);
  await expectUsable(apply);

  await apply.click();
  await expect(title).toBeHidden();
  expect(await app.calls("respond_to_action")).toEqual([
    {
      cmd: "respond_to_action",
      args: { sessionId: "session-x", actionId: "note-1", decision: { decision: "approve" } },
    },
  ]);
});

function importEntry(index: number): SshConfigPreviewEntryDto {
  const origin = { file: "/home/me/.ssh/config", line: index * 4 + 1, block: `host-${index}` };
  return {
    index,
    name: `host-${index}`,
    group: 0,
    host: { value: `host-${index}.example.test`, origin },
    port: { value: 22, origin: null },
    username: { value: "deploy", origin },
    tags: [],
    identityFile: null,
    jumpHost: null,
    conflict: null,
  };
}

test("SSH config import with many hosts: the list scrolls, title and buttons stay usable", async ({ app }) => {
  const preview: SshConfigImportPreviewDto = {
    groups: [{ name: "config", parent: null, sourcePath: "/home/me/.ssh/config" }],
    entries: Array.from({ length: 80 }, (_, i) => importEntry(i)),
    files: [{ path: "/home/me/.ssh/config", depth: 0, status: "read" }],
    skipped: [],
    identityFilePaths: [],
  };
  await app.launch({ ...populated(), commands: { preview_ssh_config_import: { result: preview } } });
  const page = app.page;

  await page.getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("button", { name: "Import …", exact: true }).click();

  const title = page.getByRole("heading", { name: "Import ssh_config" });
  const dialog = page.locator("div.flex-col", { has: title }).last();
  const lastHost = dialog.getByText("host-79.example.test");
  await expect(lastHost).toBeAttached();
  const cancel = dialog.getByRole("button", { name: "Cancel", exact: true });
  const confirm = dialog.getByRole("button", { name: "Import", exact: true });

  await expectUsable(title);
  await expectUsable(cancel);
  await expectUsable(confirm);
  await expectOnlyContentScrolls(await scrollContainerWithin(dialog, lastHost), [title, cancel, confirm]);
  await expectUsable(confirm);

  await cancel.click();
  await expect(title).toBeHidden();
  expect(await app.calls("apply_ssh_config_import")).toEqual([]);
});

test("connection step log: a long log scrolls with the page, the navigation stays usable", async ({ app }) => {
  await app.launch({
    settings: { ...ACKNOWLEDGED },
    servers: [server({ id: "s-far", name: "far-away", jumpHost: "hop-0" })],
    commands: {
      connect: {
        error: {
          message: "Authentication failed",
          code: null,
          connect_log: failingStepLog(12),
        },
      },
    },
  });
  const page = app.page;
  await page.getByRole("button", { name: /^far-away\b/ }).click();
  const log = page.getByTestId("connect-step-log");
  await log.locator("summary").click();
  const failedStep = log.locator("li[data-failed=true]");
  await expect(failedStep).toBeVisible();

  const manage = page.getByRole("button", { name: "Manage", exact: true });
  const settings = page.getByRole("button", { name: "Settings", exact: true });
  const scroller = page.locator("main.overflow-y-auto");
  await expectUsable(manage);
  await expectUsable(settings);
  await expectOnlyContentScrolls(scroller, [manage, settings]);

  // The end of the log, with its Copy button, is reachable by scrolling.
  const copy = log.getByRole("button", { name: "Copy", exact: true });
  await copy.scrollIntoViewIfNeeded();
  await expectUsable(copy);
  await expectUsable(settings);
});
