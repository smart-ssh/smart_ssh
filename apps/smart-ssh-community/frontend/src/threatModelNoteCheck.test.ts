// @vitest-environment node
//
// Issue #864: `scripts/check-threat-model-note.mjs` requires a THREAT-MODEL.md
// change or a reasoned "no change" line for PRs touching security-critical paths.
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import {
  findThreatModelProblems,
  loadSecurityPaths,
} from "../../../../scripts/check-threat-model-note.mjs";

const prefixes = loadSecurityPaths();
const run = (files: string[], body = "") => findThreatModelProblems(files, body, prefixes);
const SEC = "crates/core/src/filter/engine.rs";

describe("check-threat-model-note", () => {
  it("fails: security path, no update, no note", () => {
    const r = run([SEC, "README.md"], "Some description");
    expect(r).not.toBeNull();
    expect(r!.matches).toEqual([SEC]);
    expect(r!.message).toContain(SEC);
    expect(r!.message).toContain("THREAT-MODEL.md");
  });
  it("passes: THREAT-MODEL.md changed", () => {
    expect(run([SEC, "THREAT-MODEL.md"])).toBeNull();
  });
  it("passes: valid note (en dash and hyphen)", () => {
    expect(run([SEC], "x\nThreat model: no change – only refactoring of names")).toBeNull();
    expect(run([SEC], "Threat model: no change - only refactoring of names\r\n")).toBeNull();
  });
  it("fails: note with empty or too short reason", () => {
    expect(run([SEC], "Threat model: no change – ")).not.toBeNull();
    expect(run([SEC], "Threat model: no change –")).not.toBeNull();
    expect(run([SEC], "Threat model: no change – short")).not.toBeNull();
  });
  it("fails: note not at line start", () => {
    expect(run([SEC], "see Threat model: no change – only refactoring of names")).not.toBeNull();
  });
  it("passes: only test files under a security path", () => {
    expect(
      run([
        "crates/core/src/filter/tests.rs",
        "crates/core/src/filter/tests_extra.rs",
        "crates/core/src/risk/engine_tests.rs",
        "crates/ssh-transport/tests/integration.rs",
        "apps/smart-ssh-community/frontend/src/components/HostKeyDialog.test.tsx",
      ]),
    ).toBeNull();
  });
  it("passes: no security path", () => {
    expect(run(["README.md", "crates/app-shell/src/lib.rs"])).toBeNull();
  });
  it("shipped security-paths.txt parses and every path exists", () => {
    expect(prefixes.length).toBeGreaterThan(10);
    const root = path.resolve(__dirname, "../../../..");
    for (const p of prefixes) {
      const dir = path.dirname(path.join(root, p));
      const exists =
        fs.existsSync(path.join(root, p)) ||
        (fs.existsSync(dir) && fs.readdirSync(dir).some((f) => path.join(dir, f).startsWith(path.join(root, p))));
      expect(exists, p).toBe(true);
    }
  });
});
