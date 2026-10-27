// Issue #696, Spec 0011: an automatically run command names the rule behind
// it; confirmed/denied commands show no such line.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { subscribeRequestOpenFilterRule } from "../navigationBus";
import type { AutoExecRule, RuleDto } from "../types";
import { ChatItemView, type ChatItem } from "./ChatPanel";

const listRulesMock = vi.fn<() => Promise<RuleDto[]>>();

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  listRules: () => listRulesMock(),
  listServers: () => Promise.resolve([{ id: "srv-1", name: "prod-1" }]),
}));

function rule(id: string): RuleDto {
  return {
    id,
    patternType: "glob",
    patternValue: "x",
    action: "Allow",
    scope: "Global",
    priority: 0,
    patternError: null,
  };
}

const globalRule: AutoExecRule = {
  ruleId: "r1",
  patternType: "glob",
  patternValue: "uptime",
  scope: "Global",
  segmentIndices: [0],
};

function item(overrides: Partial<Extract<ChatItem, { type: "action" }>>): ChatItem {
  return {
    type: "action",
    id: "item-1",
    actionId: "action-1",
    action: { SuggestCommand: { command: "uptime" } },
    decision: "AutoExec",
    responded: false,
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
}

function renderItem(i: ChatItem) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ChatItemView
        item={i}
        onRespond={vi.fn()}
        onAcceptWithRule={vi.fn()}
        onExport={vi.fn()}
        onContinueTruncated={vi.fn()}
        serverId="srv-1"
        sessionId="session-1"
      />
    </I18nextProvider>,
  );
}

describe("auto-exec rule line (issue #696)", () => {
  beforeEach(() => listRulesMock.mockResolvedValue([rule("r1"), rule("r2")]));
  afterEach(() => vi.clearAllMocks());

  it("shows pattern and scope and opens the rule on click", async () => {
    const requested: string[] = [];
    const off = subscribeRequestOpenFilterRule((id) => requested.push(id));
    renderItem(item({ autoExecRules: [globalRule] }));
    const link = await screen.findByRole("button", { name: "uptime" });
    expect(link.closest("p")?.textContent).toContain("Automatisch ausgeführt");
    expect(link.closest("p")?.textContent).toContain("global");
    await act(async () => {
      fireEvent.click(link);
    });
    expect(requested).toEqual(["r1"]);
    off();
  });

  it("lists the rule of each segment with server and tag scopes", async () => {
    renderItem(
      item({
        autoExecRules: [
          globalRule,
          { ...globalRule, ruleId: "r2", patternValue: "ls *", scope: { Server: "srv-1" } },
          { ...globalRule, ruleId: "r3", patternValue: "df *", scope: { Tag: "web" } },
        ],
      }),
    );
    expect(await screen.findByText(/Server prod-1/)).toBeTruthy();
    expect(screen.getByText(/Tag web/)).toBeTruthy();
    expect(screen.getAllByText(/Regel/).length).toBe(3);
  });

  it("renders a rule that no longer exists as plain text", async () => {
    // The existing rule renders as a button only after the rule list was loaded, so
    // waiting for it proves the "gone" rule was checked against the loaded list.
    renderItem(
      item({
        autoExecRules: [{ ...globalRule, ruleId: "gone" }, { ...globalRule, ruleId: "r1", patternValue: "ls *" }],
      }),
    );
    expect(await screen.findByRole("button", { name: "ls *" })).toBeInTheDocument();
    expect(screen.getAllByText("uptime").some((el) => el.tagName === "SPAN")).toBe(true);
    expect(screen.queryByRole("button", { name: "uptime" })).toBeNull();
  });

  it("shows no line for confirmed or denied commands", () => {
    const { unmount } = renderItem(
      item({
        decision: { Confirm: { reason: "r", code: "FILTER_NO_RULE_MATCHED" } },
        autoExecRules: [globalRule],
      }),
    );
    expect(screen.queryByText(/Automatisch ausgeführt/)).toBeNull();
    unmount();
    renderItem(
      item({ decision: { Deny: { reason: "r", code: "FILTER_HARD_BLACKLIST" } }, autoExecRules: [globalRule] }),
    );
    expect(screen.queryByText(/Automatisch ausgeführt/)).toBeNull();
  });

  it("shows no line when no rules came with the event", () => {
    renderItem(item({ autoExecRules: [] }));
    expect(screen.queryByText(/Automatisch ausgeführt/)).toBeNull();
  });
});
