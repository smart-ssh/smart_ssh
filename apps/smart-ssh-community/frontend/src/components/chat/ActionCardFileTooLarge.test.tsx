// Issue #1559: write-proposal preview note for a file over the preview cap.
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../../testI18n";
import type { ChatItem } from "./chatItem";
import { ActionCard } from "./ActionCard";

vi.mock("../../api/api", async () => (await import("../../test/apiMock")).createApiMock({}));

type ActionItem = Extract<ChatItem, { type: "action" }>;

function renderCard(overrides: Partial<ActionItem>) {
  const item: ActionItem = {
    type: "action",
    id: "item-1",
    actionId: "action-1",
    action: { WriteRemoteFile: { path: "/etc/big.log", content: "new" } },
    decision: { Confirm: { reason: "r", code: "c" } },
    responded: false,
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    previousFileContent: null,
    previousFileSize: null,
    previousFileTooLarge: false,
    targetName: null,
    riskAssessment: null,
    riskSecondOpinionPending: false,
    secondOpinionUnavailable: false,
    startedAt: null,
    origin: { kind: "internal" },
    ...overrides,
  };
  return render(
    <I18nextProvider i18n={testI18n}>
      <ActionCard item={item} onRespond={vi.fn()} serverId="srv-1" sessionId="session-1" />
    </I18nextProvider>,
  );
}

describe("ActionCard write preview of an over-cap file (issue #1559)", () => {
  it("shows the too-large hint and not the binary hint", () => {
    renderCard({ previousFileTooLarge: true });
    expect(screen.getByText(/Datei ist größer als 1 MiB, kein Textvergleich\. Neue Größe: 3 B\./)).toBeTruthy();
    expect(screen.queryByText(/Binärdatei/)).toBeNull();
  });

  it("keeps the binary hint with the real size for a binary file", () => {
    renderCard({ previousFileSize: 2048 });
    expect(screen.getByText(/Binärdatei — kein Textvergleich möglich\. Bisherige Größe: 2\.0 KB/)).toBeTruthy();
    expect(screen.queryByText(/größer als 1 MiB/)).toBeNull();
  });
});
