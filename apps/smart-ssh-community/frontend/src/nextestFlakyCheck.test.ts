// @vitest-environment node
// Issue #901: `scripts/check-nextest-flaky.mjs` flags tests that failed and
// then passed on a retry in the nextest JUnit report.
import { describe, expect, it } from "vitest";
import { findFlakyTests } from "../../../../scripts/check-nextest-flaky.mjs";

const clean = `<testsuite><testcase name="ok" classname="a::b" time="0.1"/></testsuite>`;
const flaky = `<testsuite>
<testcase name="ok" classname="a::b" time="0.1"/>
<testcase name="wobbly" classname="a::c" time="0.2">
<flakyFailure message="boom"><system-out>x</system-out></flakyFailure>
</testcase>
</testsuite>`;

describe("check-nextest-flaky", () => {
  it("reports nothing for a clean report", () => {
    expect(findFlakyTests(clean)).toEqual([]);
  });
  it("reports a test that failed and then passed", () => {
    expect(findFlakyTests(flaky)).toEqual(["a::c::wobbly"]);
  });
  it("does not report a plain failure", () => {
    const failed = `<testcase name="bad" classname="a::d"><failure message="x"/></testcase>`;
    expect(findFlakyTests(failed)).toEqual([]);
  });
});
