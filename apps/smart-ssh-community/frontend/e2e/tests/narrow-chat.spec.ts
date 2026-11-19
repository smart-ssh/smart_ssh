import { expect, test } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { populated } from "../support/data";
import { expectFullyInViewport, expectNoHorizontalPageScroll, expectUsable } from "../support/layout";
import { captureScreen } from "../support/screenshots";
import { chatInput, openSession, redCommandProposal } from "../support/session";

// Issue #583, Spec 0113 §4a: the chat panel in the narrow layout.
const LONG_USER = "please check the disk usage of ".concat("/srv/data/very/long/path/segment ".repeat(6));
const WIDE_CODE = `\`\`\`bash\n${"tar czf /tmp/backup.tgz /srv/data/very/long/path ".repeat(4)}\n\`\`\``;
const OPTIONS = [
  { label: "Alpha", description: "first option" },
  { label: "Beta", description: null },
];

test.describe("narrow chat panel @phone", () => {
  test("messages, cards and confirmation fit; the composer works", async ({ app }) => {
    await app.launch({
      ...populated(),
      commands: {
    explain_confirmation: { result: { segments: [
      { index: 0, text: "rm -rf /var/lib/app/cache", state: "needsRule", rule: null, reasons: [] },
      { index: 1, text: "echo done", state: "covered", rule: { patternType: "glob", patternValue: "echo *" }, reasons: [] },
    ], suggestions: [] } },
    explain_segment_rules: { result: { segmentIndex: 0, segmentText: "rm -rf /var/lib/app/cache", blockedReasons: [], suggestions: [
      { kind: "exact", segmentVerdict: { covered: true, reasons: [] }, commandVerdict: { autoExec: true, reasons: [] }, names: [], patterns: [{ patternType: "exact", patternValue: "rm -rf /var/lib/app/cache" }] },
      { kind: "subcommand", segmentVerdict: { covered: true, reasons: [] }, commandVerdict: { autoExec: true, reasons: [] }, names: ["rm -rf"], patterns: [{ patternType: "glob", patternValue: "rm -rf /var/lib/app/*" }] },
    ] } },
    create_quick_rules_batch: { result: ["rule-1"] },
  },
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
    await expectNoHorizontalPageScroll(page);

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

    // Inline expansion (no popup): suggestions inside the viewport, tall
    // touch targets; staging a rule neither executes nor saves it.
    expect(await page.getByRole("button", { name: /Accept \+ Rule/ }).count()).toBe(0);
    const toggle = page.getByRole("button", { name: /^Rule suggestions for rm -rf/ });
    await expectUsable(toggle);
    expect((await toggle.boundingBox())!.height).toBeGreaterThanOrEqual(44);
    await toggle.click();
    const suggestion = page.getByRole("button", { name: /rm -rf \/var\/lib\/app\/\*/ });
    await expectFullyInViewport(suggestion);
    await expectNoHorizontalPageScroll(page);
    await captureScreen(page, "chat-narrow-quick-rule");
    await suggestion.click();
    await expect(page.getByTestId("staged-rule")).toBeVisible();
    await expectNoHorizontalPageScroll(page);
    const save = page.getByRole("button", { name: "Save rules (1)" });
    await expectUsable(save);
    await save.click();
    await expect(page.getByTestId("saved-rule")).toBeVisible();
    expect((await app.calls("create_quick_rules_batch")).length).toBe(1);
    expect((await app.calls("respond_to_action")).length).toBe(0);

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
    await expectNoHorizontalPageScroll(page);
    await captureScreen(page, "chat-narrow-cards");
  });
});
