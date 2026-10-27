import { describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { flakyLines, summarySection } from "./report-flaky.mjs";

const script = path.join(path.dirname(fileURLToPath(import.meta.url)), "report-flaky.mjs");
const cwd = "/work/frontend";

const t = (projectName, status) => ({ projectName, status, results: [] });
const report = {
  config: { rootDir: `${cwd}/e2e/tests` },
  suites: [
    {
      title: "a.spec.ts",
      file: "a.spec.ts",
      specs: [{ title: "top level", file: "a.spec.ts", line: 3, tests: [t("webkit-small", "expected")] }],
      suites: [
        {
          title: "filter rules",
          file: "a.spec.ts",
          specs: [
            { title: "fails", file: "a.spec.ts", line: 10, tests: [t("chromium-small", "unexpected")] },
            {
              title: "Tab order, long values",
              file: "a.spec.ts",
              line: 100,
              tests: [t("webkit-large", "flaky"), t("chromium-large", "expected")],
            },
          ],
        },
      ],
    },
  ],
};

describe("report-flaky", () => {
  it("lists exactly the flaky test in the fixed format", () => {
    expect(flakyLines(report, cwd)).toEqual([
      "flaky: e2e/tests/a.spec.ts:100 filter rules › Tab order, long values [webkit-large]",
    ]);
  });

  it("writes no summary section without flaky tests", () => {
    expect(summarySection([])).toBe("");
    const clean = { ...report, suites: [{ ...report.suites[0], suites: [] }] };
    expect(flakyLines(clean, cwd)).toEqual([]);
  });

  it("puts the lines in one fenced block", () => {
    const s = summarySection(["flaky: x:1 y [p]"]);
    expect(s).toContain("## Flaky browser tests");
    expect(s).toContain("```\nflaky: x:1 y [p]\n```");
  });

  it("CLI: annotation, file and summary; missing report is a warning with exit 0", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "flaky-"));
    const rep = path.join(dir, "r.json");
    const out = path.join(dir, "out.txt");
    const sum = path.join(dir, "sum.md");
    fs.writeFileSync(rep, JSON.stringify({ ...report, config: { rootDir: path.join(dir, "e2e/tests") } }));
    const stdout = execFileSync("node", [script, rep, out], {
      cwd: dir,
      env: { ...process.env, GITHUB_STEP_SUMMARY: sum },
      encoding: "utf8",
    });
    const line = "flaky: e2e/tests/a.spec.ts:100 filter rules › Tab order, long values [webkit-large]";
    expect(stdout).toContain(`::warning title=Flaky browser test::${line}`);
    expect(fs.readFileSync(out, "utf8")).toBe(`${line}\n`);
    expect(fs.readFileSync(sum, "utf8")).toContain("## Flaky browser tests");

    const none = execFileSync("node", [script, path.join(dir, "missing.json"), out], {
      cwd: dir,
      env: { ...process.env, GITHUB_STEP_SUMMARY: sum },
      encoding: "utf8",
    });
    expect(none).toContain("::warning");
  });
});
