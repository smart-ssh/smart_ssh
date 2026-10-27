// Issue #696: the filter rules tab opens a requested rule for editing.
import { render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { RuleDto } from "../types";
import { FilterRulesView } from "./FilterRulesView";

const rules: RuleDto[] = [
  { id: "r1", patternType: "glob", patternValue: "uptime", action: "Allow", scope: "Global", priority: 0, patternError: null },
  { id: "r2", patternType: "glob", patternValue: "df *", action: "Allow", scope: "Global", priority: 1, patternError: null },
];

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  listRules: () => Promise.resolve(rules),
  listServers: () => Promise.resolve([]),
  listKnownTags: () => Promise.resolve([]),
  listHardBlacklist: () => Promise.resolve([]),
}));

function renderView(initialRuleId: string | null, onConsumed = vi.fn()) {
  render(
    <I18nextProvider i18n={testI18n}>
      <FilterRulesView initialRuleId={initialRuleId} onInitialRuleConsumed={onConsumed} />
    </I18nextProvider>,
  );
  return onConsumed;
}

describe("FilterRulesView initialRuleId (issue #696)", () => {
  it("opens the requested rule in the edit form", async () => {
    const consumed = renderView("r2");
    await waitFor(() => expect(consumed).toHaveBeenCalled());
    expect((await screen.findByDisplayValue("df *")) as HTMLInputElement).toBeTruthy();
  });

  it("ignores an unknown rule id but still consumes the request", async () => {
    const consumed = renderView("gone");
    await waitFor(() => expect(consumed).toHaveBeenCalled());
    // The rule list is rendered (loaded) before the absence of an opened form means anything.
    expect(await screen.findByText("glob: df *")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("df *")).toBeNull();
  });
});
