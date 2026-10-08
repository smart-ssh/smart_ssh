// Accessibility scans with axe-core. A scan fails on every `serious` or
// `critical` violation, except those in the allowlist below. Each entry
// names the rule, where it applies and why it is not fixed yet.
import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";
import { expect } from "./app";

interface AllowedViolation {
  /** axe rule id. */
  rule: string;
  /** Screens the entry applies to (the `screen` passed to `expectNoSeriousA11yViolations`), or `*`. */
  screens: "*" | string[];
  reason: string;
}

export const AXE_ALLOWLIST: AllowedViolation[] = [
  {
    rule: "color-contrast",
    screens: "*",
    reason:
      "The dark theme uses muted text colours (secondary labels, inactive tabs, hints) below the 4.5:1 ratio on many screens. Raising them is an app-wide visual design change, not part of the test setup.",
  },
  {
    rule: "scrollable-region-focusable",
    screens: ["settings: MCP Server"],
    reason:
      "The client configuration snippet is a scrollable <pre> that cannot receive keyboard focus. Making a non-interactive element focusable conflicts with the repository's jsx-a11y/no-noninteractive-tabindex lint rule and needs its own design (e.g. a copy button or a focusable code view).",
  },
];

export async function expectNoSeriousA11yViolations(page: Page, screen: string): Promise<void> {
  const results = await new AxeBuilder({ page }).analyze();
  const blocking = results.violations
    .filter((v) => v.impact === "serious" || v.impact === "critical")
    .filter(
      (v) =>
        !AXE_ALLOWLIST.some(
          (entry) => entry.rule === v.id && (entry.screens === "*" || entry.screens.includes(screen)),
        ),
    )
    .map((v) => ({
      rule: v.id,
      impact: v.impact,
      help: v.help,
      nodes: v.nodes.map((n) => n.target.join(" ")),
    }));
  expect(blocking, `serious/critical accessibility violations on ${screen}`).toEqual([]);
}
