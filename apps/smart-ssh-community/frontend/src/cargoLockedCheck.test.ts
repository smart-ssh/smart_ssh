// @vitest-environment node
//
// Issue #822: every Cargo build/test/clippy/tree call in the CI workflows and
// in `scripts/gate.sh` uses `--locked`, so CI tests exactly the checked-in
// `Cargo.lock` instead of re-resolving a stale one.
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const REPO_ROOT = path.resolve(__dirname, "../../../..");
const CARGO_CALL = /\bcargo\s+(build|test|clippy|tree)\b/;

/**
 * Returns the offending lines (1-based number + text): a Cargo call without
 * `--locked`. Comment lines, YAML `name:` labels and `echo`/`step` messages
 * mention commands without running them and are ignored.
 */
export function findUnlockedCargoCalls(text: string): string[] {
  const bad: string[] = [];
  text.split("\n").forEach((line, i) => {
    const t = line.trim();
    if (t.startsWith("#")) return;
    if (/^(- )?name:/.test(t) || /^(echo|step)\b/.test(t)) return;
    if (CARGO_CALL.test(t) && !/--locked\b/.test(t)) bad.push(`${i + 1}: ${t}`);
  });
  return bad;
}

function files(): string[] {
  const out = ["scripts/gate.sh"];
  for (const dir of [".github/workflows", ".github/workflow-proposals"]) {
    const abs = path.join(REPO_ROOT, dir);
    if (!fs.existsSync(abs)) continue;
    for (const f of fs.readdirSync(abs)) {
      if (f.endsWith(".yml") || f.endsWith(".yaml")) out.push(`${dir}/${f}`);
    }
  }
  return out;
}

describe("cargo --locked guard", () => {
  it("covers the gate script and the workflows", () => {
    const list = files();
    expect(list).toContain("scripts/gate.sh");
    expect(list).toContain(".github/workflows/community.yml");
  });

  it.each(files())("%s: every cargo build/test/clippy/tree uses --locked", (rel) => {
    const text = fs.readFileSync(path.join(REPO_ROOT, rel), "utf8");
    expect(findUnlockedCargoCalls(text)).toEqual([]);
  });

  it("counter-proof: flags a call without --locked", () => {
    expect(findUnlockedCargoCalls("        run: cargo test --workspace\n")).toHaveLength(1);
    expect(findUnlockedCargoCalls("  tree_output=$(cargo tree -p x)")).toHaveLength(1);
    expect(findUnlockedCargoCalls("        run: cargo clippy --locked -- -D warnings")).toEqual([]);
    expect(findUnlockedCargoCalls("# cargo build is mentioned here")).toEqual([]);
  });
});
