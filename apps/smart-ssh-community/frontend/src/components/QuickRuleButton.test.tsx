// Spec 0011, sections 2-4: a quick-rule suggestion is a set of patterns that
// is accepted together, and its label is translated from kind + parameters.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { suggestRulePatterns } from "../api";
import type { PatternSuggestionDto } from "../types";
import { testI18n } from "../testI18n";
import { QuickRuleButton } from "./ChatPanel";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  suggestRulePatterns: vi.fn(),
}));

const SUGGESTIONS: PatternSuggestionDto[] = [
  {
    kind: "exactPerSegment",
    names: [],
    patterns: [
      { patternType: "exact", patternValue: "ps aux" },
      { patternType: "exact", patternValue: "grep java" },
    ],
  },
  {
    kind: "subcommand",
    names: ["systemctl status"],
    patterns: [
      { patternType: "exact", patternValue: "systemctl status" },
      { patternType: "glob", patternValue: "systemctl status *" },
    ],
  },
];

async function open(lang: string) {
  await testI18n.changeLanguage(lang);
  vi.mocked(suggestRulePatterns).mockResolvedValue(SUGGESTIONS);
  const onAccept = vi.fn();
  render(
    <I18nextProvider i18n={testI18n}>
      <QuickRuleButton
        actionId="a1"
        command="ps aux | grep java"
        wasEdited={false}
        serverId="srv-1"
        onAccept={onAccept}
      />
    </I18nextProvider>,
  );
  fireEvent.click(screen.getAllByRole("button")[0]);
  await waitFor(() => expect(screen.getByText(/ps aux, grep java/)).toBeTruthy());
  return onAccept;
}

afterEach(async () => {
  await testI18n.changeLanguage("de");
});

describe("QuickRuleButton", () => {
  it("accepts a multi-pattern suggestion as one set with the chosen scope", async () => {
    const onAccept = await open("en");
    fireEvent.click(screen.getByText("Exact per part: ps aux, grep java"));
    expect(onAccept).toHaveBeenCalledWith(
      "a1",
      SUGGESTIONS[0].patterns,
      { Server: "srv-1" },
      null,
    );
  });

  it("renders translated labels in English and German", async () => {
    await open("en");
    expect(screen.getByText("All `systemctl status` calls: systemctl status, systemctl status *")).toBeTruthy();
    expect(screen.queryByText(/Alle/)).toBeNull();
  });

  it("renders German labels", async () => {
    await open("de");
    expect(screen.getByText("Exakt je Teilkommando: ps aux, grep java")).toBeTruthy();
    expect(screen.getByText("Alle `systemctl status`-Aufrufe: systemctl status, systemctl status *")).toBeTruthy();
  });
});
