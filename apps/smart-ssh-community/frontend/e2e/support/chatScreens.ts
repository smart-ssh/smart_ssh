// Chat panel states shared by the accessibility scan, the screenshots and the
// keyboard/layout tests of the chat review (issue #383).
import type { AppDriver } from "./app";
import { expect } from "./app";
import { populated } from "./data";
import { openSession, redCommandProposal } from "./session";

export const LONG_URL = `https://docs.example.com/${"very-long-path-segment/".repeat(8)}index.html`;
export const LONG_COMMAND_LINE = `tar czf /tmp/backup.tgz ${"/srv/data/very/long/path/segment ".repeat(10).trim()}`;

/** A session with an assistant answer, a web research card, a document, a
 * truncated answer and an error in the chat. Returns the session id. */
export async function openChatWithMessages(app: AppDriver): Promise<string> {
  await app.launch(populated());
  const sessionId = await openSession(app, "db-primary");
  await app.emit("chat-text-delta", {
    sessionId,
    delta: `Here is a long answer with a path ${LONG_URL} and some **markdown**.\n\n- one\n- two`,
  });
  await app.emit("chat-web-activity", {
    sessionId,
    activity: {
      kind: "search",
      input: "how to rotate logs on a very long server name ".repeat(3),
      results: [],
      cited: [{ title: "Log rotation guide", url: LONG_URL }],
      contentTruncated: true,
      errorCode: null,
    },
  });
  await app.emit("chat-document-generated", {
    sessionId,
    actionId: "doc-1",
    title: "Runbook",
    contentMarkdown: "# Runbook\n\n1. step one\n2. step two",
  });
  await app.emit("chat-response-truncated", { sessionId });
  await app.emit("chat-error", { sessionId, message: "Provider unreachable", code: null });
  await expect(app.page.getByText("Provider unreachable")).toBeVisible();
  return sessionId;
}

/** A pending red command with a long line, and a command that is shown
 * without an editor. */
export async function openChatWithConfirmation(app: AppDriver): Promise<string> {
  await app.launch({ ...populated(), commands: { suggest_rule_patterns: { result: [
    { label: "rm -rf /var/lib/app/cache", patternType: "Exact", patternValue: "rm -rf /var/lib/app/cache" },
    { label: "rm -rf /var/lib/app/*", patternType: "Glob", patternValue: "rm -rf /var/lib/app/*" },
  ] } } });
  const sessionId = await openSession(app, "db-primary");
  await app.emit(
    "chat-action-proposed",
    redCommandProposal(sessionId, `rm -rf /var/lib/app/cache\n${LONG_COMMAND_LINE}`),
  );
  await expect(app.page.getByRole("button", { name: "Execute", exact: true })).toBeVisible();
  return sessionId;
}
