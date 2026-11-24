// Issue #1466: exit-code text of the action card — in the header of an
// auto-executed card and in the result line of every other card.
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../../testI18n";
import type { ActionResultPayload } from "../../api/types";
import type { ChatItem } from "./chatItem";
import { ActionCard } from "./ActionCard";

vi.mock("../../api/api", async () => (await import("../../test/apiMock")).createApiMock({}));

type ActionItem = Extract<ChatItem, { type: "action" }>;

function commandResult(overrides: Partial<Extract<ActionResultPayload, { kind: "command" }>>): ActionResultPayload {
  return {
    kind: "command",
    command: "uptime",
    stdout: "",
    stderr: "",
    exitCode: 0,
    cancelled: false,
    truncated: false,
    ...overrides,
  };
}

function renderCard(overrides: Partial<ActionItem>) {
  const item: ActionItem = {
    type: "action",
    id: "item-1",
    actionId: "action-1",
    action: { SuggestCommand: { command: "uptime" } },
    decision: { Confirm: { reason: "r", code: "c" } },
    responded: true,
    userDecision: "approve",
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    previousFileContent: null,
    previousFileSize: null,
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

describe("ActionCard exit-code text (issue #1466)", () => {
  it("shows the translated exit-code line in the result of a non-auto-exec card", () => {
    renderCard({ result: commandResult({ exitCode: 0 }) });
    expect(screen.getAllByText("Exit-Code: 0")).toHaveLength(1);
  });

  it("shows a dash when the exit code is missing", () => {
    renderCard({ result: commandResult({ exitCode: null }) });
    expect(screen.getAllByText("Exit-Code: —")).toHaveLength(1);
  });

  it("shows the exit code of an auto-exec card in the header only", () => {
    const { container } = renderCard({
      decision: "AutoExec",
      responded: false,
      userDecision: undefined,
      result: commandResult({ exitCode: 3 }),
    });
    const matches = screen.getAllByText("Exit-Code: 3");
    expect(matches).toHaveLength(1);
    expect(container.querySelector("div.border-b")?.contains(matches[0])).toBe(true);
  });

  it("shows no exit-code line for a cancelled result", () => {
    renderCard({ result: commandResult({ exitCode: null, cancelled: true }) });
    expect(screen.queryByText(/^Exit-Code: /)).toBeNull();
  });
});
