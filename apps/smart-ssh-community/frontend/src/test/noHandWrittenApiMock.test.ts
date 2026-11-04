// Guard (issue #908): no test file defines a hand-written factory for the
// `api` module. Use `createApiMock({...overrides})` from `./apiMock` instead
// (issue #906), so a new export of `api.ts` is mocked automatically.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = join(__dirname, "..");

// Every `vi.mock("<relative>/api", <factory>)` must build its module with
// `createApiMock` — sync or async, with or without `importActual`. The factory
// is read up to the parenthesis that closes the `vi.mock(` call.
const API_MOCK_START = /vi\.mock\(\s*["'](?:\.\.?\/)+api["']/g;

function factoryText(source: string, from: number): string {
  let depth = 1;
  for (let i = from; i < source.length; i++) {
    const c = source[i];
    if (c === "(") depth++;
    else if (c === ")" && --depth === 0) return source.slice(from, i);
  }
  return source.slice(from);
}

export function findHandWrittenApiMock(source: string): boolean {
  for (const m of source.matchAll(API_MOCK_START)) {
    if (!factoryText(source, m.index + m[0].length).includes("createApiMock")) {
      return true;
    }
  }
  return false;
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
    expect(
      findHandWrittenApiMock(
        ['vi.mock("./api",', "() => ({ a: vi.fn() }));"].join(" "),
      ),
    ).toBe(true);
  });

  it("fails on an async hand-written factory", () => {
    const fixture = [
      'vi.mock("./api", async () => {',
      '  const actual = await vi.importActual<typeof import("./api")>("./api");',
      "  return { commandErrorCode: actual.commandErrorCode, connect: vi.fn() };",
      "});",
    ].join("\n");
    expect(findHandWrittenApiMock(fixture)).toBe(true);
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
