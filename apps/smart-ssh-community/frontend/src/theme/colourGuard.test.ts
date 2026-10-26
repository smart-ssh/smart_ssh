// Guard (issue #805): no hard-coded colours in components. Colours come from
// semantic tokens (theme/dark.css, THEMING.md). Files that are not migrated yet
// are listed in ALLOWLIST; the list may only shrink: once a listed file is
// clean, this test fails until the entry is removed.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { findHardCodedColours } from "./colourGuard";
import ALLOWLIST from "./colourAllowlist.json";

const SRC = join(__dirname, "..");

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) return sourceFiles(p);
    if (!/\.(ts|tsx)$/.test(name) || /\.(test|spec)\.tsx?$/.test(name)) return [];
    return [p];
  });
}

describe("colour guard", () => {
  const files = sourceFiles(SRC)
    .map((p) => relative(SRC, p).split("\\").join("/"))
    .filter((f) => !f.startsWith("test/") && !f.startsWith("theme/"));

  it("detects palette classes, hex and rgb(a) in fixtures", () => {
    expect(findHardCodedColours('<a className="text-slate-400 hover:bg-red-300/50" />')).toHaveLength(2);
    expect(findHardCodedColours('const c = "#6c7cf5";')).toEqual(["#6c7cf5"]);
    expect(findHardCodedColours("x = `rgba(0,0,0,.5)`")).toHaveLength(1);
    expect(findHardCodedColours('"bg-black/70 text-white"')).toHaveLength(2);
    expect(findHardCodedColours('"bg-surface text-fg-muted border-border"')).toEqual([]);
    expect(findHardCodedColours("// Issue #696 and bg-slate-400\nconst a = 1;")).toEqual([]);
  });

  it("has no hard-coded colours outside the allowlist", () => {
    const offenders = files
      .filter((f) => !ALLOWLIST.includes(f))
      .filter((f) => findHardCodedColours(readFileSync(join(SRC, f), "utf8")).length > 0);
    expect(offenders, "use semantic tokens (THEMING.md)").toEqual([]);
  });

  it("lists only files that still need the exemption", () => {
    const stale = ALLOWLIST.filter(
      (f) =>
        !files.includes(f) ||
        findHardCodedColours(readFileSync(join(SRC, f), "utf8")).length === 0,
    );
    expect(stale, "remove these files from colourAllowlist.json").toEqual([]);
  });

  it("keeps the migrated base off the allowlist", () => {
    const base = ["App.tsx", ...files.filter((f) => f.startsWith("components/ui/"))];
    expect(ALLOWLIST.filter((f) => base.includes(f))).toEqual([]);
  });
});
