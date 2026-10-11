// Shared fixtures for the notes panel of the server form.
import { expect, type AppDriver } from "./app";
import type { CannedResponse } from "../harness/fixtureTypes";

/** Two revisions, so that the history has a diff and an original version. */
export const NOTE_COMMANDS: Record<string, CannedResponse> = {
  list_note_revisions: {
    result: [
      { id: "r1", content: "first line", editedBy: { kind: "user" }, createdAt: "2026-01-01T10:00:00Z" },
      {
        id: "r2",
        content: "first line\nsecond line",
        editedBy: { kind: "ai", provider: "anthropic", model: "a-model-with-a-rather-long-name-v1" },
        createdAt: "2026-01-02T10:00:00Z",
      },
    ],
  },
};

/** Opens the server form of db-primary, where the notes panel lives. */
export async function openNotes(app: AppDriver): Promise<void> {
  const page = app.page;
  await page.getByRole("button", { name: "Manage", exact: true }).click();
  await page.getByRole("button", { name: "🖥️ db-primary" }).click();
  await expect(page.getByLabel("Note", { exact: false }).first()).toBeVisible();
}
