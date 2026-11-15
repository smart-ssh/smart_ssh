// @vitest-environment node
//
// Issue #1346: `scripts/check-spec-index.mjs` keeps every spec's `Status:` and `Zweck:` line
// present and `docs/specs/INDEX.md` generated.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { buildIndex, checkSpecIndex, findSpecProblems } from "../../../../scripts/check-spec-index.mjs";

const spec = (title: string, purpose = "Worum es geht.") =>
  `# Spec ${title}\n\nStatus: umgesetzt\n${purpose ? `Zweck: ${purpose}\n` : ""}Bezüge: Spec 0001.\n\n## 1. x\n`;
const valid = () =>
  new Map([
    ["0001-first.md", spec("0001 — First")],
    ["0002-second.md", spec("0002 — Second", "Zeile eins\nund Zeile zwei.")],
  ]);
const run = (files: Map<string, string>, index: string | null = buildIndex(files)) =>
  findSpecProblems(files, index).map((p) => p.message);

describe("check-spec-index", () => {
  it("passes for the real docs/specs", () => {
    expect(checkSpecIndex()).toEqual([]);
  });

  it("lists number, title, status and purpose, joining a wrapped purpose", () => {
    const index = buildIndex(valid());
    expect(index).toContain("| 0001 | [First](0001-first.md) | umgesetzt | Worum es geht. |");
    expect(index).toContain("Zeile eins und Zeile zwei.");
  });

  it("accepts a valid folder", () => {
    expect(run(valid())).toEqual([]);
  });

  it("fails on a missing Zweck line", () => {
    const f = valid();
    f.set("0003-none.md", spec("0003 — None", ""));
    expect(run(f).some((m) => /missing `Zweck:`/.test(m))).toBe(true);
  });

  it("fails on a missing Status line", () => {
    const f = valid();
    f.set("0003-none.md", "# Spec 0003 — None\n\nZweck: x\n");
    expect(run(f).some((m) => /missing `Status:`/.test(m))).toBe(true);
  });

  it("fails on a duplicate number and a bad file name", () => {
    const f = valid();
    f.set("0001-other.md", spec("0001 — Other"));
    f.set("5-bad_name.md", spec("5 — Bad"));
    const msgs = run(f);
    expect(msgs.some((m) => /duplicate spec number 0001/.test(m))).toBe(true);
    expect(msgs.some((m) => /file name does not match/.test(m))).toBe(true);
  });

  it("fails when a spec is added, renamed or its Zweck changes without regenerating", () => {
    const f = valid();
    const index = buildIndex(f);
    const added = new Map(f).set("0003-new.md", spec("0003 — New"));
    expect(run(added, index).some((m) => /out of date/.test(m))).toBe(true);
    const renamed = new Map(f);
    renamed.set("0001-renamed.md", renamed.get("0001-first.md")!);
    renamed.delete("0001-first.md");
    expect(run(renamed, index).some((m) => /out of date/.test(m))).toBe(true);
    const changed = new Map(f).set("0001-first.md", spec("0001 — First", "Anders."));
    expect(run(changed, index).some((m) => /out of date/.test(m))).toBe(true);
    expect(run(f, null).some((m) => /missing; /.test(m))).toBe(true);
  });

  it("writes an index that then passes, on a custom dir and index path", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "spec-"));
    try {
      for (const [n, c] of valid()) fs.writeFileSync(path.join(dir, n), c);
      const index = path.join(dir, "custom-index.md");
      expect(checkSpecIndex({ dir, index }).length).toBeGreaterThan(0);
      expect(checkSpecIndex({ dir, index, writeIndex: true })).toEqual([]);
      expect(checkSpecIndex({ dir, index })).toEqual([]);
      expect(fs.readFileSync(index, "utf8")).toContain("do not edit");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
