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

export const AXE_ALLOWLIST: AllowedViolation[] = [];

export async function expectNoSeriousA11yViolations(page: Page, screen: string): Promise<void> {
  // axe measures the colour that is rendered at that moment. A running
  // opacity/colour transition (e.g. the fade-in of the chat message toolbar)
  // blends the text with the background and gives a false `color-contrast`
  // finding, so wait until no finite animation is running (issue #461).
  // Endless ones (spinners, pulses) never finish and are ignored.
  await expect
    .poll(() => page.evaluate(() => document
          .getAnimations()
          .filter((a) => a.playState === "running" && a.effect?.getComputedTiming().iterations !== Infinity).length), {
      message: "animations still running before the accessibility scan",
    })
    .toBe(0);
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
