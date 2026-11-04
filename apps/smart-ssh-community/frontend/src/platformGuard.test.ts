// @vitest-environment node
//
// Issue #915: platform detection lives in `platform.ts`. No other non-test
// frontend file may read `navigator.platform` or `navigator.userAgent`.
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const SRC = __dirname;
const ALLOWED = "lib/platform.ts";

/** Returns the `navigator.platform` / `navigator.userAgent` reads (line + text) in `text`. */
export function findPlatformSniffing(text: string): string[] {
  const bad: string[] = [];
  for (const m of text.matchAll(/navigator\s*\.\s*(platform|userAgent)\b/g)) {
    const line = text.slice(0, m.index).split("\n").length;
    bad.push(`${line}: ${m[0]}`);
  }
  return bad;
}

function sourceFiles(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) return sourceFiles(p);
    return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [p] : [];
  });
}

describe("platform detection is in one module (issue #915)", () => {
  it("only platform.ts reads navigator.platform / navigator.userAgent", () => {
    const offenders = sourceFiles(SRC)
      .filter((f) => path.relative(SRC, f) !== ALLOWED)
      .flatMap((f) =>
        findPlatformSniffing(fs.readFileSync(f, "utf8")).map((b) => `${path.relative(SRC, f)}:${b}`),
      );
    expect(offenders).toEqual([]);
  });

  it("counter-check: detects a fixture using navigator.platform elsewhere", () => {
    expect(findPlatformSniffing('const m = navigator.platform.includes("mac");')).toHaveLength(1);
    expect(findPlatformSniffing("const ua = navigator .userAgent;")).toHaveLength(1);
    expect(findPlatformSniffing("const l = navigator.language;")).toEqual([]);
  });
});
