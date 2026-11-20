// Issue #596, #1357, Spec 0011: the confirmation card summarises the
// sub-commands existing rules already allow, lists the others as rows that
// expand to per-segment suggestions, stages picked rules and saves them in one
// batch without resolving the confirmation.
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createQuickRulesBatch,
  explainConfirmation,
  explainSegmentRules,
  respondToAction,
} from "../api/api";
import { onChatActionOutput, onChatActionProposed } from "../api/events";
import type {
  ChatActionProposedEvent,
  ConfirmationExplanationDto,
  SegmentRuleSuggestionsDto,
} from "../api/types";
import { testI18n } from "../testI18n";
import { renderChatPanel } from "../test/renderChatPanel";

import "../test/scrollTo";

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock(),
);

vi.mock("../api/events", async () =>
  (await import("../test/eventsMock")).createEventsMock(),
);

vi.mock("../settings/riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() =>
    Promise.resolve({ enabled: false, providerId: null }),
  ),
}));

function handlerOf<T>(listener: unknown): (event: T) => void {
  const calls = vi.mocked(listener as (h: (event: T) => void) => unknown).mock
    .calls;
  return calls[calls.length - 1][0];
}

const COMMAND = "grep a f | grep -v b | wc -l";

const EXPLANATION: ConfirmationExplanationDto = {
  segments: [
    {
      index: 0,
      text: "grep a f",
      state: "covered",
      rule: { patternType: "glob", patternValue: "grep *" },
      reasons: [],
    },
    {
      index: 1,
      text: "grep -v b",
      state: "covered",
      rule: { patternType: "glob", patternValue: "grep *" },
      reasons: [],
    },
    { index: 2, text: "wc -l", state: "needsRule", rule: null, reasons: [] },
    { index: 3, text: "ls -la", state: "needsRule", rule: null, reasons: [] },
  ],
  suggestions: [],
};

const WC_RULE = [{ patternType: "glob", patternValue: "wc *" }] as const;
const LS_RULE = [{ patternType: "glob", patternValue: "ls *" }] as const;

function suggestionsFor(
  index: number,
  text: string,
  patterns: readonly { patternType: "glob"; patternValue: string }[],
  names: string[],
  autoExec: boolean,
): SegmentRuleSuggestionsDto {
  return {
    segmentIndex: index,
    segmentText: text,
    blockedReasons: [],
    suggestions: [
      {
        kind: "tool",
        names,
        patterns: [...patterns],
        segmentVerdict: { covered: true, reasons: [] },
        commandVerdict: autoExec
          ? { autoExec: true, reasons: [] }
          : { autoExec: false, reasons: [{ code: "FILTER_OUTPUT_REDIRECTION", segmentIndex: 2 }] },
      },
    ],
  };
}

async function renderCard(
  explanation: ConfirmationExplanationDto,
  reject = false,
  code = "FILTER_NO_RULE_MATCHED",
) {
  if (reject)
    vi.mocked(explainConfirmation).mockRejectedValue(new Error("boom"));
  else vi.mocked(explainConfirmation).mockResolvedValue(explanation);
  await testI18n.changeLanguage("en");
  renderChatPanel();
  await waitFor(() => expect(onChatActionOutput).toHaveBeenCalled());
  const event: ChatActionProposedEvent = {
    sessionId: "session-1",
    actionId: "action-1",
    action: { SuggestCommand: { command: COMMAND } },
    decision: {
      Confirm: { reason: "no rule", code },
    },
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    autoExecRules: [],
    previousFileContent: null,
    previousFileSize: null,
    targetName: null,
    riskAssessment: null,
    origin: { kind: "internal" },
  };
  act(() => handlerOf<ChatActionProposedEvent>(onChatActionProposed)(event));
}

describe("confirmation card explanation (issue #596)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(createQuickRulesBatch).mockResolvedValue([]);
    vi.mocked(explainSegmentRules).mockImplementation((_s, _c, index) =>
      Promise.resolve(
        index === 2
          ? suggestionsFor(2, "wc -l", WC_RULE, ["wc"], false)
          : suggestionsFor(3, "ls -la", LS_RULE, ["ls"], true),
      ),
    );
  });
  afterEach(async () => {
    await testI18n.changeLanguage("de");
  });

  it("lists each segment with its coverage state", async () => {
    await renderCard(EXPLANATION);
    expect(await screen.findByText("grep -v b")).toBeInTheDocument();
    // Already allowed: compact summary with the rule as tooltip.
    expect(screen.getByText("Already allowed")).toBeInTheDocument();
    expect(screen.getAllByTitle("covered by rule grep *")).toHaveLength(2);
    expect(screen.getAllByText("needs a rule")).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: /^Rule suggestions for/ })).toHaveLength(2);
    expect(screen.queryByText(/Accept \+ Rule/)).toBeNull();
  });

  it("shows the translated text for FILTER_RULES_UNAVAILABLE (issue #836)", async () => {
    await renderCard(
      { segments: [], suggestions: [] },
      false,
      "FILTER_RULES_UNAVAILABLE",
    );
    expect(
      await screen.findByText(/The filter rules could not be loaded/),
    ).toBeInTheDocument();
  });

  it("shows nothing extra when the backend returns no segments", async () => {
    await renderCard({ segments: [], suggestions: [] });
    // The card renders nothing while loading and when the answer is empty, so the
    // DOM cannot signal "done": let the component's own `.then` run first.
    await waitFor(() => expect(explainConfirmation).toHaveBeenCalled());
    await act(async () => {
      await vi.mocked(explainConfirmation).mock.results[0].value;
    });
    expect(screen.queryByText("Parts of this command")).toBeNull();
  });

  it("a single command shows one row with a toggle and no 'Accept + Rule'", async () => {
    await renderCard({
      segments: [{ index: 0, text: "ls -la", state: "needsRule", rule: null, reasons: [] }],
      suggestions: [],
    });
    const toggle = await screen.findByRole("button", { name: "Rule suggestions for ls -la" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText(/Accept \+ Rule/)).toBeNull();
  });

  it("expanding a row shows its suggestions with This Server preselected; the toggle is keyboard-operable", async () => {
    await renderCard(EXPLANATION);
    const toggle = await screen.findByRole("button", { name: "Rule suggestions for wc -l" });
    // A native button: focusable and operable with Enter/Space by construction.
    expect(toggle.tagName).toBe("BUTTON");
    toggle.focus();
    expect(toggle).toHaveFocus();
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(await screen.findByText(/All `wc` calls/)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "This Server" })).toBeChecked();
    expect(screen.getByText(/will still ask|covers this part/)).toBeInTheDocument();
    expect(explainSegmentRules).toHaveBeenLastCalledWith(
      "session-1",
      COMMAND,
      2,
      { Server: expect.any(String) },
      [],
    );
  });

  it("stages two rules with their scopes, saves them in one batch and keeps the dialog open", async () => {
    await renderCard(EXPLANATION);
    fireEvent.click(await screen.findByRole("button", { name: "Rule suggestions for wc -l" }));
    fireEvent.click(await screen.findByText(/All `wc` calls/));
    // Row stays in place, shows the staged rule; the command is not executed.
    expect(await screen.findByTestId("staged-rule")).toHaveTextContent("wc * · This Server");
    expect(screen.getByRole("button", { name: "Rule suggestions for wc -l" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    fireEvent.click(screen.getByRole("button", { name: "Rule suggestions for ls -la" }));
    fireEvent.click(await screen.findByRole("radio", { name: "Global" }));
    fireEvent.click(await screen.findByText(/All `ls` calls/));
    await waitFor(() => expect(screen.getAllByTestId("staged-rule")).toHaveLength(2));
    // The other row's suggestions consider the staged rule.
    expect(vi.mocked(explainSegmentRules).mock.calls.some((c) => c[4].length === 1)).toBe(true);
    expect(screen.getByText("Rules not saved: 2")).toBeInTheDocument();
    const save = screen.getByRole("button", { name: "Save rules (2)" });
    fireEvent.click(save);
    await waitFor(() => expect(createQuickRulesBatch).toHaveBeenCalledTimes(1));
    const [rules] = vi.mocked(createQuickRulesBatch).mock.calls[0];
    expect(rules).toEqual([
      { patterns: [...WC_RULE], scope: { Server: expect.any(String) } },
      { patterns: [...LS_RULE], scope: "Global" },
    ]);
    expect(respondToAction).not.toHaveBeenCalled();
    // Green "covered" marks, rows and toggles stay, dialog still open.
    expect(await screen.findAllByTestId("saved-rule")).toHaveLength(2);
    expect(screen.queryByText(/Rules not saved/)).toBeNull();
    expect(screen.getByRole("button", { name: "Rule suggestions for wc -l" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save rules (0)" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: /^Execute/ }));
    await waitFor(() =>
      expect(respondToAction).toHaveBeenCalledWith("session-1", "action-1", { decision: "approve" }),
    );
  });

  it("a staged rule can be changed and removed again; N follows", async () => {
    await renderCard(EXPLANATION);
    fireEvent.click(await screen.findByRole("button", { name: "Rule suggestions for wc -l" }));
    fireEvent.click(await screen.findByText(/All `wc` calls/));
    expect(await screen.findByRole("button", { name: "Save rules (1)" })).toBeEnabled();
    // Change: re-open the row and pick it with another scope.
    fireEvent.click(screen.getByRole("button", { name: "Rule suggestions for wc -l" }));
    fireEvent.click(await screen.findByRole("radio", { name: "Global" }));
    fireEvent.click(await screen.findByText(/All `wc` calls/));
    await waitFor(() => expect(screen.getByTestId("staged-rule")).toHaveTextContent("wc * · Global"));
    expect(screen.getByRole("button", { name: "Save rules (1)" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(await screen.findByRole("button", { name: "Save rules (0)" })).toBeDisabled();
    expect(screen.queryByTestId("staged-rule")).toBeNull();
  });

  it("a save error shows a message and keeps the staging", async () => {
    vi.mocked(createQuickRulesBatch).mockRejectedValue(new Error("disk full"));
    await renderCard(EXPLANATION);
    fireEvent.click(await screen.findByRole("button", { name: "Rule suggestions for wc -l" }));
    fireEvent.click(await screen.findByText(/All `wc` calls/));
    fireEvent.click(await screen.findByRole("button", { name: "Save rules (1)" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/could not be saved/);
    expect(screen.getByTestId("staged-rule")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save rules (1)" })).toBeEnabled();
  });

  it("a coded save error is shown translated, without the library text", async () => {
    vi.mocked(createQuickRulesBatch).mockRejectedValue({
      code: "FILTER_RULE_PATTERN_INVALID",
      message: "regex parse error at 3",
    });
    await renderCard(EXPLANATION);
    fireEvent.click(await screen.findByRole("button", { name: "Rule suggestions for wc -l" }));
    fireEvent.click(await screen.findByText(/All `wc` calls/));
    fireEvent.click(await screen.findByRole("button", { name: "Save rules (1)" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(/Invalid pattern in a filter rule/);
    expect(alert).not.toHaveTextContent(/regex parse error/);
  });

  it("Deny with staged rules saves nothing", async () => {
    await renderCard(EXPLANATION);
    fireEvent.click(await screen.findByRole("button", { name: "Rule suggestions for wc -l" }));
    fireEvent.click(await screen.findByText(/All `wc` calls/));
    expect(await screen.findByText("Rules not saved: 1")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Deny/ }));
    await waitFor(() =>
      expect(respondToAction).toHaveBeenCalledWith("session-1", "action-1", { decision: "deny" }),
    );
    expect(createQuickRulesBatch).not.toHaveBeenCalled();
  });

  it("shows the always-asks reason for a single rule-covered command", async () => {
    await renderCard({
      segments: [
        {
          index: 0,
          text: "cat /etc//shadow",
          state: "blocked",
          rule: null,
          reasons: ["FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM"],
        },
      ],
      suggestions: [],
    });
    expect(await screen.findByText("cat /etc//shadow")).toBeInTheDocument();
    expect(
      screen.getByText(
        /always asks, whatever the rules: Reads a file with credentials/,
      ),
    ).toBeInTheDocument();
  });

  it("says so when the coverage lookup fails; Confirm and Reject still work", async () => {
    vi.mocked(respondToAction).mockClear();
    await renderCard(EXPLANATION, true);
    expect(
      await screen.findByText("Rule coverage could not be loaded"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Execute/ }));
    await waitFor(() =>
      expect(respondToAction).toHaveBeenCalledWith("session-1", "action-1", {
        decision: "approve",
      }),
    );
  });
});
