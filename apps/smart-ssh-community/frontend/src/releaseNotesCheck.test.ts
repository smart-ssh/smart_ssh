// @vitest-environment node
//
// Issue #500: `scripts/check-release-notes.mjs` validates `release-notes/X.Y.Z.json`
// (Spec 0048): schema, `en` equals the CHANGELOG Highlights, file per released version.
import { describe, expect, it } from "vitest";
import {
  checkReleaseNotes,
  findReleaseNotesProblems,
} from "../../../../scripts/check-release-notes.mjs";

const EN = ["One", "Two", "Three"];
const changelog = (extra = "") =>
  `# Changelog\n\n## [Unreleased]\n\n## [0.7.0] — 2026-11-01\n\n### Highlights\n- One\n- Two\n- Three\n\n### Added\n- x\n${extra}`;
const note = (over: Record<string, unknown> = {}) => ({
  version: "0.7.0",
  highlights: { en: EN, de: ["Eins", "Zwei", "Drei"], fr: ["Un", "Deux", "Trois"], es: ["Uno", "Dos", "Tres"] },
  ...over,
});
const ser = (o: unknown) => `${JSON.stringify(o, null, 2)}\n`;
const run = (content: string, name = "0.7.0.json", cl = changelog()) =>
  findReleaseNotesProblems(new Map([[name, content]]), cl);
const hl = (patch: Record<string, unknown>) => note({ highlights: { ...note().highlights, ...patch } });
const expectFail = (problems: { file: string; message: string }[], re: RegExp) => {
  expect(problems.length).toBeGreaterThan(0);
  expect(problems.some((p) => re.test(p.message))).toBe(true);
  expect(problems[0].file).toContain("0.7.0");
};

describe("check-release-notes", () => {
  it("passes for the real repository", () => {
    expect(checkReleaseNotes()).toEqual([]);
  });

  it("accepts a valid file", () => {
    expect(run(ser(note()))).toEqual([]);
  });

  it("fails on invalid JSON", () => expectFail(run("{nope"), /invalid JSON/));
  it("fails on a missing language", () => {
    const n = note();
    delete (n.highlights as Record<string, unknown>).fr;
    expectFail(run(ser(n)), /missing language key "fr"/);
  });
  it("fails on an extra language", () =>
    expectFail(run(ser(hl({ it: EN }))), /unexpected language key "it"/));
  it("fails on an extra top-level key", () =>
    expectFail(run(ser(note({ extra: 1 }))), /top-level keys/));
  it("fails with fewer than 3 or more than 6 items", () => {
    expectFail(run(ser(hl({ de: ["a", "b"] }))), /allowed are 3 to 6/);
    expectFail(run(ser(hl({ de: ["a", "b", "c", "d", "e", "f", "g"] }))), /allowed are 3 to 6/);
  });
  it("fails on different item counts", () =>
    expectFail(run(ser(hl({ de: ["a", "b", "c", "d"] }))), /different item counts/));
  it("fails on a multi-line item", () =>
    expectFail(run(ser(hl({ de: ["a\nb", "b", "c"] }))), /several lines/));
  it("fails on an empty item", () =>
    expectFail(run(ser(hl({ de: ["a", "  ", "c"] }))), /empty/));
  it("fails on Markdown markup", () => {
    for (const bad of ["**bold**", "`code`", "- item", "[a](http://x)", "# head", "_emph_"]) {
      expectFail(run(ser(hl({ de: [bad, "b", "c"] }))), /Markdown/);
    }
  });
  it("allows underscores inside words", () =>
    expect(run(ser(hl({ de: ["ssh_config", "b", "c"] })))).toEqual([]));
  it("fails when version does not match the file name", () =>
    expectFail(run(ser(note({ version: "0.7.1" }))), /does not match the file name/));
  it("fails for a version without a CHANGELOG section", () => {
    const n = note({ version: "0.9.0" });
    const p = findReleaseNotesProblems(new Map([["0.9.0.json", ser(n)]]), changelog());
    expect(p.some((x) => /no section in CHANGELOG/.test(x.message) && x.file === "0.9.0.json")).toBe(true);
  });
  it("fails when en differs from the Highlights", () =>
    expectFail(run(ser(hl({ en: ["One", "Two", "Other"] }))), /"en" differs/));
  it("fails for a released version after 0.5.2 without a file", () => {
    const p = findReleaseNotesProblems(new Map(), changelog());
    expect(p).toEqual([{ file: "release-notes/0.7.0.json", message: expect.stringMatching(/missing/) }]);
  });
  it("does not require files for 0.5.2 and older", () => {
    const old = "\n## [0.5.2] — 2026-10-01\n\n### Added\n- x\n";
    const p = findReleaseNotesProblems(new Map([["0.7.0.json", ser(note())]]), changelog(old));
    expect(p).toEqual([]);
  });
  it("fails without trailing newline", () =>
    expectFail(run(JSON.stringify(note(), null, 2)), /trailing newline/));
});
