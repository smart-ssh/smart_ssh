// @vitest-environment node
//
// Issue #850: `scripts/check-adr.mjs` keeps ADR numbers unique, statuses in one
// vocabulary and `INDEX.md` generated.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { buildIndex, checkAdr, findAdrProblems } from "../../../../scripts/check-adr.mjs";

const adr = (title: string, status: string) => `# ${title}\n\n## Status\n\n${status}\n\n## Kontext\n\nx\n`;
const valid = () =>
  new Map([
    ["0001-first.md", adr("0001-first", "Accepted · Spec 0001")],
    ["0002-second.md", adr("ADR 0002 — Second", "Superseded by 0003")],
    ["0003-third.md", adr("0003 — Third", "Proposed")],
    ["0004-fourth.md", adr("0004-fourth", "Deprecated")],
  ]);
const run = (files: Map<string, string>, index: string | null = buildIndex(files)) =>
  findAdrProblems(files, index).map((p) => p.message);

describe("check-adr", () => {
  it("passes for the real docs/adr", () => {
    expect(checkAdr()).toEqual([]);
  });

  it("accepts a valid folder", () => {
    expect(run(valid())).toEqual([]);
  });

  it("fails on a duplicate number", () => {
    const f = valid();
    f.set("0001-other.md", adr("0001-other", "Accepted"));
    expect(run(f).some((m) => /duplicate ADR number 0001/.test(m))).toBe(true);
  });

  it("fails on a missing status", () => {
    const f = valid();
    f.set("0005-none.md", "# 0005-none\n\n## Kontext\n\nx\n");
    expect(run(f).some((m) => /missing or unknown status/.test(m))).toBe(true);
  });

  it("fails on an unknown status word", () => {
    const f = valid();
    f.set("0005-odd.md", adr("0005-odd", "akzeptiert"));
    expect(run(f).some((m) => /missing or unknown status/.test(m))).toBe(true);
  });

  it("fails on a successor that does not exist", () => {
    const f = valid();
    f.set("0005-gone.md", adr("0005-gone", "Superseded by 0099"));
    expect(run(f).some((m) => /no ADR with that number/.test(m))).toBe(true);
  });

  it("fails on a bad file name", () => {
    const f = valid();
    f.set("5-bad_name.md", adr("bad", "Accepted"));
    expect(run(f).some((m) => /file name does not match/.test(m))).toBe(true);
  });

  it("fails on a stale or missing index", () => {
    const f = valid();
    const index = buildIndex(f);
    f.set("0005-new.md", adr("0005-new", "Accepted"));
    expect(run(f, index).some((m) => /out of date/.test(m))).toBe(true);
    expect(run(f, null).some((m) => /missing/.test(m))).toBe(true);
  });

  it("writes an index that then passes, on a custom dir and index path", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "adr-"));
    try {
      for (const [n, c] of valid()) fs.writeFileSync(path.join(dir, n), c);
      const index = path.join(dir, "custom-index.md");
      expect(checkAdr({ dir, index }).length).toBeGreaterThan(0);
      expect(checkAdr({ dir, index, writeIndex: true })).toEqual([]);
      expect(checkAdr({ dir, index })).toEqual([]);
      expect(fs.readFileSync(index, "utf8")).toContain("do not edit");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
