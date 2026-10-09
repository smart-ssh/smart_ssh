// @vitest-environment node
//
// Issue #106: `scripts/check-changelog-highlights.mjs` verlangt für jede
// veröffentlichte Version nach dem Stichtag `### Highlights` mit 3 bis 6
// einzeiligen Punkten (Spec 0048 §2).
import { describe, expect, it } from "vitest";
import {
  checkChangelog,
  CUTOFF_VERSION,
  findHighlightProblems,
} from "../../../../scripts/check-changelog-highlights.mjs";

const items = (n: number) =>
  Array.from({ length: n }, (_, i) => `- Item ${i + 1}`).join("\n");
const section = (version: string, body: string) =>
  `## [${version}] — 2026-11-01\n\n${body}\n\n### Added\n- x\n`;
const file = (...sections: string[]) => `# Changelog\n\n## [Unreleased]\n\n${sections.join("\n")}`;

describe("check-changelog-highlights", () => {
  it("passes for the real CHANGELOG.md", () => {
    expect(checkChangelog()).toEqual([]);
  });

  it("accepts 3 and 6 items", () => {
    for (const n of [3, 6]) {
      const text = file(section("0.5.3", `### Highlights\n${items(n)}`));
      expect(findHighlightProblems(text)).toEqual([]);
    }
  });

  it("accepts a plain hyphen in the heading", () => {
    const text = file(`## [0.5.3] - 2026-11-01\n\n### Highlights\n${items(3)}\n`);
    expect(findHighlightProblems(text)).toEqual([]);
  });

  it("fails without Highlights", () => {
    const text = file(section("0.5.3", "### Added\n- y"));
    expect(findHighlightProblems(text)).toHaveLength(1);
  });

  it("fails when Highlights is not the first heading", () => {
    const text = file(`## [0.5.3] — 2026-11-01\n\n### Added\n- y\n\n### Highlights\n${items(3)}\n`);
    expect(findHighlightProblems(text)).toHaveLength(1);
  });

  it("fails with 2 or 7 items", () => {
    for (const n of [2, 7]) {
      const text = file(section("0.5.3", `### Highlights\n${items(n)}`));
      expect(findHighlightProblems(text)).toHaveLength(1);
    }
  });

  it("fails with a multi-line item", () => {
    const text = file(section("0.5.3", `### Highlights\n${items(3)}\n  continued here`));
    expect(findHighlightProblems(text).length).toBeGreaterThan(0);
  });

  it("exempts the cut-off version, older versions and Unreleased", () => {
    const text = file(section(CUTOFF_VERSION, "### Added\n- y"), section("0.4.0", "### Added\n- y"));
    expect(findHighlightProblems(text)).toEqual([]);
  });

  it("compares versions numerically", () => {
    const text = file(section("0.10.0", "### Added\n- y"));
    expect(findHighlightProblems(text)).toHaveLength(1);
  });
});
