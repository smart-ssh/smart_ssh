import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { populated } from "../support/data";
import { expectFullyInViewport, expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";
import { chatInput, openSession, redCommandProposal } from "../support/session";

// Issue #583, Spec 0113 §4a: the chat panel in the narrow layout.
const LONG_USER = "please check the disk usage of ".concat("/srv/data/very/long/path/segment ".repeat(6));
const WIDE_CODE = `\`\`\`bash\n${"tar czf /tmp/backup.tgz /srv/data/very/long/path ".repeat(4)}\n\`\`\``;
const OPTIONS = [
  { label: "Alpha", description: "first option" },
  { label: "Beta", description: null },
];

async function noPageScroll(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
}

test.describe("narrow chat panel @phone", () => {
  test("messages, cards and confirmation fit; the composer works", async ({ app }) => {
    await app.launch({
      ...populated(),
      commands: { accept_and_create_rule: { result: null }, explain_confirmation: { result: { segments: [], suggestions: [
        { kind: "exact", verdict: { autoExec: true, reasons: [] }, names: [], patterns: [{ patternType: "exact", patternValue: "rm -rf /var/lib/app/cache" }] },
        { kind: "subcommand", verdict: { autoExec: true, reasons: [] }, names: ["rm -rf"], patterns: [{ patternType: "glob", patternValue: "rm -rf /var/lib/app/*" }] },
      ] } } },
    });
    const page = app.page;
    await page.evaluate(() => {
      const ext = (window as unknown as { __e2eExtensions: { registerComposerAction: (a: unknown) => void; registerDocumentAction: (a: unknown) => void } }).__e2eExtensions;
      for (const id of ["attach", "dictate"]) {
        ext.registerComposerAction({ id, label: `Action ${id}`, icon: "★", onInvoke: () => { (window as unknown as Record<string, number>)[`__${id}`] = 1; } });
      }
      ext.registerDocumentAction({ id: "doc-a", label: "Extension document action with a long label", onInvoke: () => {} });
    });
    const sessionId = await openSession(app, "db-primary");

    await chatInput(page).fill(LONG_USER);
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await app.emit("chat-text-delta", { sessionId, delta: `Here is the command:\n\n${WIDE_CODE}` });
    await app.emit("chat-document-generated", { sessionId, actionId: "doc-1", title: "Runbook with a rather long title for a phone", contentMarkdown: "# Runbook\n\n1. step one" });
    await app.emit("chat-question-asked", { sessionId, questionId: "q1", question: "Which one?", options: OPTIONS, multiSelect: false });
    await app.emit("chat-web-activity", { sessionId, activity: { kind: "search", input: "how to rotate logs on a long server ".repeat(3), results: [], cited: [{ title: "Log rotation guide", url: `https://docs.example.com/${"long-segment/".repeat(10)}` }], contentTruncated: false, errorCode: null } });
    await app.emit("chat-action-proposed", redCommandProposal(sessionId, `rm -rf /var/lib/app/cache\ntar czf /tmp/backup.tgz ${"/srv/data/very/long/path/segment ".repeat(6)}`));
    await expect(page.getByRole("button", { name: "Execute", exact: true })).toBeVisible();
    await noPageScroll(page);

    // The code block scrolls inside its box.
    const pre = page.locator("pre").first();
    expect(await pre.evaluate((el) => el.scrollWidth > el.clientWidth && ["auto", "scroll"].includes(getComputedStyle(el).overflowX))).toBe(true);

    // Full command stays visible and the confirmation buttons are tall enough.
    await page.getByRole("button", { name: "Execute", exact: true }).scrollIntoViewIfNeeded();
    for (const name of ["Execute", "Deny"]) {
      const b = page.getByRole("button", { name, exact: true });
      await expectUsable(b);
      expect((await b.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    }
    await captureScreen(page, "chat-narrow-confirmation");

    // Quick rule panel inside the viewport; picking a suggestion works.
    await page.getByRole("button", { name: "Accept + Rule" }).click();
    const suggestion = page.getByRole("button", { name: /rm -rf \/var\/lib\/app\/\*/ });
    await expectFullyInViewport(suggestion);
    await noPageScroll(page);
    await captureScreen(page, "chat-narrow-quick-rule");
    await suggestion.click();
    expect((await app.calls("accept_and_create_rule")).length).toBe(1);

    // Composer: both extension actions behind "+", send reachable.
    await expectFullyInViewport(chatInput(page));
    await expectUsable(page.getByRole("button", { name: "Send", exact: true }));
    await page.getByRole("button", { name: "More input actions" }).click();
    await expectUsable(page.getByRole("button", { name: "Action attach" }));
    await expectUsable(page.getByRole("button", { name: "Action dictate" }));
    await page.getByRole("button", { name: "Action dictate" }).click();
    expect(await page.evaluate(() => (window as unknown as Record<string, number>).__dictate)).toBe(1);
    await captureScreen(page, "chat-narrow-composer");
    await expectNoSeriousA11yViolations(page, "chat narrow");
  });

  test("document actions and question card fit", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    await page.evaluate(() => {
      const ext = (window as unknown as { __e2eExtensions: { registerDocumentAction: (a: unknown) => void } }).__e2eExtensions;
      ext.registerDocumentAction({ id: "doc-a", label: "Extension document action with a long label", onInvoke: () => {} });
    });
    const sessionId = await openSession(app, "db-primary");
    await app.emit("chat-document-generated", { sessionId, actionId: "doc-1", title: "Runbook", contentMarkdown: "# Runbook" });
    await app.emit("chat-question-asked", { sessionId, questionId: "q1", question: "Which one?", options: OPTIONS, multiSelect: false });
    for (const name of [/Save as Markdown/, /Extension document action/, /Alpha/]) {
      const b = page.getByRole("button", { name }).first();
      await b.scrollIntoViewIfNeeded();
      await expectUsable(b);
    }
    await noPageScroll(page);
    await captureScreen(page, "chat-narrow-cards");
  });
});
