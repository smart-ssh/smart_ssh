// @vitest-environment node
//
// Issue #862: the webview can only read the settings store. No non-test
// frontend file may call a store method that writes.
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const SRC = __dirname;

/** Returns the store write calls (1-based line + text) in `text`. */
export function findStoreWrites(text: string): string[] {
  const bad: string[] = [];
  for (const m of text.matchAll(/\w*[sS]tore\w*\s*\.\s*(set|delete|clear|reset|save)\s*\(/g)) {
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

describe("the settings store is read-only for the webview (issue #862)", () => {
  it("no non-test source writes to a store", () => {
    const offenders = sourceFiles(SRC).flatMap((f) =>
      findStoreWrites(fs.readFileSync(f, "utf8")).map((b) => `${path.relative(SRC, f)}:${b}`),
    );
    expect(offenders).toEqual([]);
  });

  it("counter-check: detects each write method in a sample", () => {
    const sample = [
      "await store.set(KEY, 1);",
      "await store.delete(KEY);",
      "await store.clear();",
      "await store.reset();",
      "await settingsStore.save();",
    ].join("\n");
    expect(findStoreWrites(sample)).toHaveLength(5);
    expect(findStoreWrites('await store.get("a"); await setUiPreference("a", 1);')).toEqual([]);
  });
});
