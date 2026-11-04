// Guard (issue #908): no test file defines a hand-written factory for the
// `api` module. Use `createApiMock({...overrides})` from `./apiMock` instead
// (issue #906), so a new export of `api.ts` is mocked automatically.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(__dirname, "..");

// `vi.mock("<relative>/api", <factory>)` whose factory is not `async`, i.e.
// `() => ({ … })`, `() => { … }` or `function () { … }`. An async factory
// that calls `createApiMock` (or spreads `importOriginal`) is allowed.
const HAND_WRITTEN = /vi\.mock\(\s*["'](?:\.\.?\/)+api["']\s*,\s*(?:\(\s*\)\s*=>|function\b)/;

export function findHandWrittenApiMock(source: string): boolean {
  return HAND_WRITTEN.test(source);
}

function testFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "node_modules" ? [] : testFiles(p);
    return /\.test\.tsx?$/.test(e.name) ? [p] : [];
  });
}

describe("no hand-written api mock", () => {
  it("fails on a fixture with a hand-written factory", () => {
    const fixture = [
      'import { vi } from "vitest";',
      'vi.mock("../api",' + " () => ({",
      "  getServer: vi.fn(),",
      "}));",
    ].join("\n");
    expect(findHandWrittenApiMock(fixture)).toBe(true);
    expect(findHandWrittenApiMock(['vi.mock("./api",', "() => ({ a: vi.fn() }));"].join(" "))).toBe(true);
  });

  it("accepts the shared createApiMock factory", () => {
    const fixture =
      'vi.mock("../api", async () =>\n  (await import("../test/apiMock")).createApiMock({}));';
    expect(findHandWrittenApiMock(fixture)).toBe(false);
  });

  it("finds no test file with a hand-written api factory", () => {
    const offenders = testFiles(SRC)
      .filter((f) => !f.endsWith("noHandWrittenApiMock.test.ts"))
      .filter((f) => findHandWrittenApiMock(readFileSync(f, "utf8")));
    expect(offenders).toEqual([]);
  });
});
