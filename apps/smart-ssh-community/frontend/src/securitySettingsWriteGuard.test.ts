// @vitest-environment node
//
// Issue #856: the security switches are written by backend commands only. No
// non-test frontend file may pass one of these keys to a store `set`/`delete`.
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const PROTECTED_KEYS = ["redRiskAlwaysConfirm", "riskClassifierEnabled", "riskClassifierProviderId"];
const SRC = __dirname;

/** Returns the offending store calls (1-based line + text) in `text`. */
export function findProtectedStoreWrites(text: string): string[] {
  // Constants that hold a protected key, e.g. `const ENABLED_KEY = "riskClassifierEnabled"`.
  const names = [...text.matchAll(/\b(\w+)\s*(?::\s*[\w<>]+\s*)?=\s*["'`](\w+)["'`]/g)]
    .filter((m) => PROTECTED_KEYS.includes(m[2]))
    .map((m) => m[1]);
  const needles = [...PROTECTED_KEYS, ...names];
  const bad: string[] = [];
  for (const m of text.matchAll(/\w*[sS]tore\w*\s*\.\s*(set|delete)\s*\(([^)]*)\)/gi)) {
    const args = m[2];
    if (needles.some((n) => new RegExp(`\\b${n}\\b`).test(args))) {
      const line = text.slice(0, m.index).split("\n").length;
      bad.push(`${line}: ${m[0]}`);
    }
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

describe("security settings are never written through the store (issue #856)", () => {
  it("no non-test source passes a protected key to store.set/delete", () => {
    const offenders = sourceFiles(SRC).flatMap((f) =>
      findProtectedStoreWrites(fs.readFileSync(f, "utf8")).map((b) => `${path.relative(SRC, f)}:${b}`),
    );
    expect(offenders).toEqual([]);
  });

  it("counter-check: detects direct and constant-based writes in a sample", () => {
    const sample = [
      'const KEY = "redRiskAlwaysConfirm";',
      "await store.set(KEY, false);",
      'await store.delete("riskClassifierEnabled");',
      'await settingsStore.set("riskClassifierProviderId", null);',
    ].join("\n");
    expect(findProtectedStoreWrites(sample)).toHaveLength(3);
    expect(findProtectedStoreWrites('await store.set("uiLanguage", "de");')).toEqual([]);
  });
});
