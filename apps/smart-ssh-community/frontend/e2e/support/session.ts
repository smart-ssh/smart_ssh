// Helpers for tests that need an open session tab.
import type { Page } from "@playwright/test";
import type { ChatActionProposedEvent } from "../../src/types";
import { expect, type AppDriver } from "./app";

interface ModelSnapshot {
  sessions: { sessionId: string; serverName: string }[];
}

/** Clicks a server in the "Connect" list and waits for its session tab. */
export async function openSession(app: AppDriver, serverName: string): Promise<string> {
  const before = (await app.calls("connect")).length;
  await app.page.getByRole("button", { name: new RegExp(`^${serverName}\\b`) }).click();
  await expect.poll(async () => (await app.calls("connect")).length).toBe(before + 1);
  await expect(chatInput(app.page)).toBeVisible();
  const snapshot = (await app.page.evaluate(() => window.__e2e!.snapshot())) as ModelSnapshot;
  const session = snapshot.sessions.find((s) => s.serverName === serverName);
  expect(session, `no session for ${serverName}`).toBeDefined();
  return session!.sessionId;
}

/** The chat input of the visible session tab. */
export function chatInput(page: Page) {
  return page.locator("form textarea").locator("visible=true");
}

/** A command the filter engine wants confirmed, with a red risk on both axes. */
export function redCommandProposal(
  sessionId: string,
  command: string,
  actionId = "action-1",
): ChatActionProposedEvent {
  return {
    sessionId,
    actionId,
    action: { SuggestCommand: { command } },
    decision: { Confirm: { reason: "Command needs confirmation", code: "CONFIRM_REQUIRED" } },
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    previousFileContent: null,
    previousFileSize: null,
    targetName: null,
    riskAssessment: {
      serverRisk: "red",
      serverRiskReason: "Deletes files recursively",
      dataRisk: "red",
      dataRiskReason: "Reads secrets",
      aiReviewed: false,
    },
    origin: { kind: "internal" },
  };
}
