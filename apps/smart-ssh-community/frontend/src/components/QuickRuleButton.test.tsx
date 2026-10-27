// Spec 0011, sections 2-4: a quick-rule suggestion is a set of patterns that
// is accepted together, and its label is translated from kind + parameters.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { explainConfirmation } from "../api";
import type { ExplainedSuggestionDto } from "../types";
import { testI18n } from "../testI18n";
import { QuickRuleButton } from "./ChatPanel";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  explainConfirmation: vi.fn(),
}));

const AUTO = { autoExec: true, reasons: [] };
const SUGGESTIONS: ExplainedSuggestionDto[] = [
  {
    kind: "exactPerSegment",
    verdict: AUTO,
    names: [],
    patterns: [
      { patternType: "exact", patternValue: "ps aux" },
      { patternType: "exact", patternValue: "grep java" },
    ],
  },
  {
    kind: "subcommand",
    verdict: {
      autoExec: false,
      reasons: [{ code: "FILTER_OUTPUT_REDIRECTION", segmentIndex: null }],
    },
    names: ["systemctl status"],
    patterns: [
      { patternType: "exact", patternValue: "systemctl status" },
      { patternType: "glob", patternValue: "systemctl status *" },
    ],
  },
];

async function open(lang: string) {
  await testI18n.changeLanguage(lang);
  vi.mocked(explainConfirmation).mockResolvedValue({ segments: [], suggestions: SUGGESTIONS });
  const onAccept = vi.fn();
  render(
    <I18nextProvider i18n={testI18n}>
      <QuickRuleButton
        sessionId="s1"
        actionId="a1"
        command="ps aux | grep java"
        wasEdited={false}
        serverId="srv-1"
        onAccept={onAccept}
      />
    </I18nextProvider>,
  );
  fireEvent.click(screen.getAllByRole("button")[0]);
  expect(await screen.findByText(/ps aux, grep java/)).toBeTruthy();
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
      AUTO,
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

  // Issue #596
  it("marks each suggestion with the backend verdict", async () => {
    await open("en");
    expect(screen.getByText("runs automatically next time")).toBeTruthy();
    expect(screen.getByText(/will still ask: .*Output redirection/)).toBeTruthy();
  });

  it("re-evaluates the verdicts when the scope changes", async () => {
    await open("en");
    vi.mocked(explainConfirmation).mockClear();
    fireEvent.click(screen.getByLabelText("Global"));
    await waitFor(() =>
      expect(explainConfirmation).toHaveBeenCalledWith("s1", "ps aux | grep java", "Global"),
    );
  });

  it("passes the verdict of the picked suggestion to the accept handler", async () => {
    const onAccept = await open("en");
    fireEvent.click(screen.getByText(/All `systemctl status` calls/));
    expect(onAccept.mock.calls[0][4].autoExec).toBe(false);
  });
});
