// Issue #696, Spec 0011: an automatically run command names the rule behind
// it; confirmed/denied commands show no such line.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { subscribeRequestOpenFilterRule } from "../buses/navigationBus";
import type { AutoExecRule, RuleDto, ServerDto } from "../api/types";
import { ChatItemView, type ChatItem } from "./ChatPanel";

const listRulesMock = vi.fn<() => Promise<RuleDto[]>>();

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    listRules: () => listRulesMock(),
    listServers: () =>
      Promise.resolve([{ id: "srv-1", name: "prod-1" } as ServerDto]),
  }),
);

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

function item(
  overrides: Partial<Extract<ChatItem, { type: "action" }>>,
): ChatItem {
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
    const header = link.closest("div");
    expect(header?.textContent).toContain("Automatisch ausgeführt");
    expect(header?.textContent).toContain("global");
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
          {
            ...globalRule,
            ruleId: "r2",
            patternValue: "ls *",
            scope: { Server: "srv-1" },
          },
          {
            ...globalRule,
            ruleId: "r3",
            patternValue: "df *",
            scope: { Tag: "web" },
          },
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
        autoExecRules: [
          { ...globalRule, ruleId: "gone" },
          { ...globalRule, ruleId: "r1", patternValue: "ls *" },
        ],
      }),
    );
    expect(
      await screen.findByRole("button", { name: "ls *" }),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText("uptime").some((el) => el.tagName === "SPAN"),
    ).toBe(true);
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
      item({
        decision: { Deny: { reason: "r", code: "FILTER_HARD_BLACKLIST" } },
        autoExecRules: [globalRule],
      }),
    );
    expect(screen.queryByText(/Automatisch ausgeführt/)).toBeNull();
  });

  it("shows the header without rules when no rules came with the event", () => {
    renderItem(item({ autoExecRules: [] }));
    expect(screen.getByText("Automatisch ausgeführt")).toBeInTheDocument();
    expect(screen.queryByText("Regel")).toBeNull();
  });

  it("shows the exit code in the header once the result exists", async () => {
    renderItem(
      item({
        autoExecRules: [globalRule],
        result: {
          kind: "command",
          command: "uptime",
          stdout: "up",
          stderr: "",
          exitCode: 3,
          cancelled: false,
          truncated: false,
        },
      }),
    );
    const code = screen.getByText("Exit-Code: 3");
    const header = code.closest("div.border-b");
    expect(header).not.toBeNull();
    expect(header?.textContent).toContain("Automatisch ausgeführt");
    expect(header?.textContent).toContain("Exit-Code: 3");
    expect(screen.getAllByText(/Exit-Code/)).toHaveLength(1);
    await screen.findByRole("button", { name: "uptime" });
  });

  it("hides the $ prompt from assistive tech and keeps it out of the command text", () => {
    const { container } = renderItem(
      item({
        autoExecRules: [globalRule],
        result: {
          kind: "command",
          command: "uptime",
          stdout: "up",
          stderr: "",
          exitCode: 0,
          cancelled: false,
          truncated: false,
        },
      }),
    );
    const prompt = container.querySelector('code > span[aria-hidden="true"]');
    expect(prompt?.textContent).toBe("$ ");
    expect(container.querySelector("code")?.className).not.toContain("before:content");
  });

  // Issue #1371: status tone + icon in the header, flat frame.
  it.each([
    ["confirmed", { decision: { Confirm: { reason: "r" } }, responded: true, userDecision: "approve" as const }, "Bestätigung nötig", "text-warning-fg"],
    ["denied by user", { decision: { Confirm: { reason: "r" } }, responded: true, userDecision: "deny" as const }, "abgelehnt", "text-danger-fg"],
    ["blocked", { decision: { Deny: { code: "x", reason: "r" } } }, "blockiert", "text-danger-fg"],
  ])("shows the %s status in the header with colour and icon", (_n, overrides, text, tone) => {
    const { container } = renderItem(item(overrides as never));
    const header = container.querySelector("div.border-b");
    const badge = header?.querySelector(`span.${tone}`);
    expect(badge).not.toBeNull();
    expect(badge?.textContent).toMatch(new RegExp(text, "i"));
    expect(badge?.querySelector("svg")).not.toBeNull();
  });

  it("renders the executed card without rounded corners or shadows", () => {
    const { container } = renderItem(item({ autoExecRules: [globalRule] }));
    const cls = Array.from(container.querySelectorAll("*")).map((e) => e.getAttribute("class") ?? "").join(" ");
    expect(cls).not.toMatch(/(^|\s)rounded(-|\s|$)/);
    expect(cls).not.toMatch(/(^|\s)shadow(-|\s|$)/);
  });
});
