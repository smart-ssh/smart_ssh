// Surfaces Playwright "flaky" results (passed only on a retry) in the CI job.
// Reads the JSON report and, per flaky test, prints
//   flaky: <spec>:<line> <title> [<project>]
// emits a GitHub warning annotation, appends a section to the job summary and
// writes the lines to a file. Plain Node, no dependency. Never fails the job
// on its own: an unreadable report only produces a warning (exit 0).
//
// Usage (from the frontend directory):
//   node e2e/report-flaky.mjs [report.json] [flaky-tests.txt]
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const DEFAULT_REPORT = "e2e/test-results/results.json";
const DEFAULT_OUT = "e2e/flaky-tests.txt";

/** Collects the flaky tests of a Playwright JSON report as formatted lines. */
export function flakyLines(report, cwd = process.cwd()) {
  const rootDir = report?.config?.rootDir;
  const lines = [];
  const walk = (suite, titles) => {
    for (const spec of suite.specs ?? []) {
      for (const test of spec.tests ?? []) {
        if (test.status !== "flaky") continue;
        const file = rootDir ? path.relative(cwd, path.join(rootDir, spec.file)) : spec.file;
        const title = [...titles, spec.title].join(" › ");
        lines.push(`flaky: ${file.split(path.sep).join("/")}:${spec.line} ${title} [${test.projectName}]`);
      }
    }
    for (const child of suite.suites ?? []) {
      // The file-level suite is titled with the spec file; it is not part of
      // the title path.
      const isFile = child.title === child.file || child.title === suite.file;
      walk(child, isFile || !child.title ? titles : [...titles, child.title]);
    }
  };
  for (const root of report?.suites ?? []) {
    walk(root, root.title && root.title !== root.file ? [root.title] : []);
  }
  return lines;
}

/** The job-summary section; empty when there is nothing flaky. */
export function summarySection(lines) {
  if (lines.length === 0) return "";
  return `## Flaky browser tests\n\n${lines.length} flaky\n\n\`\`\`\n${lines.join("\n")}\n\`\`\`\n`;
}

function main() {
  const reportPath = process.argv[2] ?? DEFAULT_REPORT;
  const outPath = process.argv[3] ?? DEFAULT_OUT;
  let report;
  try {
    report = JSON.parse(fs.readFileSync(reportPath, "utf8"));
  } catch (err) {
    console.log(`::warning title=Flaky browser tests::Cannot read ${reportPath}: ${err.message}`);
    return;
  }
  const lines = flakyLines(report);
  for (const line of lines) {
    console.log(line);
    console.log(`::warning title=Flaky browser test::${line}`);
  }
  fs.writeFileSync(outPath, lines.length ? `${lines.join("\n")}\n` : "");
  const summary = summarySection(lines);
  if (summary && process.env.GITHUB_STEP_SUMMARY) {
    fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
