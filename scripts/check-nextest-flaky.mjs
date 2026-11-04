#!/usr/bin/env node
// Issue #901: fail when the nextest JUnit report contains a test that failed
// at least once and then passed on a retry (`<flakyFailure>` / `<flakyError>`).
// Usage: node scripts/check-nextest-flaky.mjs [path/to/junit.xml]
import fs from "node:fs";

const DEFAULT = "target/nextest/ci/junit.xml";

/** Returns "classname::name" of every testcase that has a flaky child element. */
export function findFlakyTests(xml) {
  const flaky = [];
  const re = /<testcase\b([^>]*?)(\/>|>([\s\S]*?)<\/testcase>)/g;
  let m;
  while ((m = re.exec(xml)) !== null) {
    const body = m[3] ?? "";
    if (!/<flaky(Failure|Error)\b/.test(body)) continue;
    const attr = (n) => (m[1].match(new RegExp(`\\b${n}="([^"]*)"`)) ?? [])[1] ?? "?";
    flaky.push(`${attr("classname")}::${attr("name")}`);
  }
  return [...new Set(flaky)];
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const file = process.argv[2] ?? DEFAULT;
  if (!fs.existsSync(file)) {
    console.error(`::error::JUnit report not found: ${file}`);
    process.exit(1);
  }
  const flaky = findFlakyTests(fs.readFileSync(file, "utf8"));
  if (flaky.length > 0) {
    console.error(`::error::${flaky.length} flaky test(s) (failed, then passed on retry):`);
    for (const t of flaky) console.error(`  ${t}`);
    process.exit(1);
  }
  console.log("No flaky tests in the JUnit report.");
}
