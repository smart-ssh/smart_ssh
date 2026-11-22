// Issue #1375: rule tables, header count, hard blacklist block, tester panel.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { EvaluationTraceDto, RuleDto } from "../api/types";
import { installMatchMediaMock, setViewportWidth } from "../test/matchMedia";
import { FilterRulesView } from "./FilterRulesView";

const narrowMock = vi.hoisted(() => ({ value: false }));
vi.mock("../hooks/useIsNarrow", () => ({ useIsNarrow: () => narrowMock.value }));

const evaluateExplainedMock = vi.fn<(...args: unknown[]) => Promise<EvaluationTraceDto>>();

const rules: RuleDto[] = [
  { id: "g1", patternType: "glob", patternValue: "ls *", action: "Allow", scope: "Global", priority: 5, patternError: null },
  { id: "s1", patternType: "glob", patternValue: "rm *", action: "Deny", scope: { Server: "srv-1" }, priority: 9, patternError: null },
  { id: "t1", patternType: "regex", patternValue: "^bad(", action: "Confirm", scope: { Tag: "production" }, priority: 1, patternError: "unclosed group" },
];

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    listRules: () => Promise.resolve(rules),
    listServers: () =>
      Promise.resolve([{ id: "srv-1", name: "web-01", tags: [] }] as never),
    listKnownTags: () => Promise.resolve([]),
    listHardBlacklist: () =>
      Promise.resolve([
        { kind: "regex", value: "rm -rf /" },
        { kind: "exact", value: "shutdown" },
      ]),
    evaluateExplained: (...args: unknown[]) => evaluateExplainedMock(...args),
  }),
);

function renderView() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <FilterRulesView />
    </I18nextProvider>,
  );
}

const seg = (index: number, matchedRule: string | null): EvaluationTraceDto => ({
  decision: "AutoExec",
  matchedRule,
  matchedHardBlacklistEntry: null,
  subCommandTraces: [],
  kind: "Segment",
  segmentIndex: index,
  checks: [],
});

beforeEach(() => {
  narrowMock.value = false;
  installMatchMediaMock();
  setViewportWidth(1280);
  evaluateExplainedMock.mockReset();
});

describe("FilterRulesView layout (issue #1375)", () => {
  it("shows the rule count in the header", async () => {
    renderView();
    await screen.findByText("ls *");
    expect(screen.getByText(/^3 (Regeln|rules)$/)).toBeInTheDocument();
  });

  it("renders one table per scope group with priority, type, pattern, action and scope", async () => {
    renderView();
    await screen.findByText("ls *");
    expect(screen.getAllByRole("table")).toHaveLength(3);
    const row = screen.getByText("rm *").closest("tr")!;
    expect(within(row).getByText("9")).toBeInTheDocument();
    expect(within(row).getByText("glob")).toBeInTheDocument();
    expect(within(row).getByText("Deny")).toBeInTheDocument();
    expect(within(row).getByText(/web-01/)).toBeInTheDocument();
  });

  it("marks an invalid pattern with a badge and keeps the library error", async () => {
    renderView();
    await screen.findByText("^bad(");
    const row = screen.getByText("^bad(").closest("tr")!;
    expect(within(row).getByText(/^(Ungültiges Muster|Invalid pattern)$/)).toBeInTheDocument();
    expect(within(row).getByRole("alert")).toHaveTextContent("unclosed group");
  });

  it("shows every hard-blacklist pattern as a chip without edit actions", async () => {
    renderView();
    const block = (await screen.findByText(/rm -rf \//)).closest("section")!;
    expect(within(block).getAllByRole("listitem")).toHaveLength(2);
    expect(within(block).getByText("shutdown")).toBeInTheDocument();
    expect(within(block).queryByRole("button")).toBeNull();
  });

  it("tests on Enter and shows the matched rule with its scope", async () => {
    evaluateExplainedMock.mockResolvedValue({
      ...seg(0, null),
      subCommandTraces: [seg(0, "g1"), seg(1, "s1")],
    });
    renderView();
    await screen.findByText("ls *");
    const input = screen.getByPlaceholderText("ls -la && rm -rf /tmp/x");
    fireEvent.change(input, { target: { value: "ls -la && rm -rf /" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(evaluateExplainedMock).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/web-01/, { selector: "p" })).toBeInTheDocument();
  });

  async function runTester(trace: EvaluationTraceDto) {
    evaluateExplainedMock.mockResolvedValue(trace);
    renderView();
    await screen.findByText("ls *");
    const input = screen.getByPlaceholderText("ls -la && rm -rf /tmp/x");
    fireEvent.change(input, { target: { value: "ls -la && rm -rf /" } });
    fireEvent.keyDown(input, { key: "Enter" });
    const label = await screen.findByText(/^(Gesamt-Entscheidung|Overall Decision)$/);
    // The overall block is the tinted container around the label.
    return label.closest("div.border") as HTMLElement;
  }

  it("shows the matched pattern per segment and the allow tint/badge for an allowed command", async () => {
    const overall = await runTester({
      ...seg(0, null),
      subCommandTraces: [seg(0, "g1")],
    });
    expect(overall).toHaveClass("bg-success-subtle", "border-success-border");
    expect(within(overall).getAllByText("AutoExec")).toHaveLength(2);
    const segment = within(overall).getByText("ls *").closest("p")!;
    expect(segment).toHaveTextContent(/^(Regel|Rule):\s*ls \*\s*\(Allow\)/);
  });

  it("shows the matched pattern and the deny tint/badge for a denied chain", async () => {
    const overall = await runTester({
      ...seg(0, null),
      decision: { Deny: { reason: "blocked by rule", code: "UNKNOWN_CODE" } },
      subCommandTraces: [
        seg(0, "g1"),
        { ...seg(1, "s1"), decision: { Deny: { reason: "blocked by rule", code: "UNKNOWN_CODE" } } },
      ],
    } as EvaluationTraceDto);
    expect(overall).toHaveClass("bg-danger-subtle", "border-danger-border");
    expect(overall).not.toHaveClass("bg-success-subtle");
    expect(within(overall).getAllByText("Deny").length).toBeGreaterThanOrEqual(2);
    const denied = within(overall).getByText("rm *").closest("p")!;
    expect(denied).toHaveTextContent(/^(Regel|Rule):\s*rm \*\s*\(Deny\)/);
    expect(denied).toHaveTextContent(/web-01/);
  });

  it("shows the confirm tint/badge", async () => {
    const overall = await runTester({
      ...seg(0, null),
      decision: { Confirm: { reason: "needs confirmation", code: "UNKNOWN_CODE" } },
    } as EvaluationTraceDto);
    expect(overall).toHaveClass("bg-warning-subtle", "border-warning-border");
    expect(within(overall).getAllByText("Confirm").length).toBeGreaterThanOrEqual(1);
  });

  it("does not render the Test rules header button on wide windows", async () => {
    renderView();
    await screen.findByText("ls *");
    expect(screen.queryByRole("button", { name: /^(Regeln testen|Test Rules|Test rules)$/i })).toBeNull();
  });

  it("narrow: the Test rules button moves focus to the tester", async () => {
    narrowMock.value = true;
    renderView();
    await screen.findByText("ls *");
    fireEvent.click(screen.getByRole("button", { name: /Test Rules|Regeln testen/i }));
    const panel = screen.getByRole("region", { name: /Test Rules|Regeln testen/i });
    expect(panel).toHaveFocus();
  });

  it("between 640 and 1099 px (stacked tester): the Test rules button moves focus to the tester", async () => {
    setViewportWidth(800);
    renderView();
    await screen.findByText("ls *");
    fireEvent.click(screen.getByRole("button", { name: /Test Rules|Regeln testen/i }));
    expect(screen.getByRole("region", { name: /Test Rules|Regeln testen/i })).toHaveFocus();
  });
});
