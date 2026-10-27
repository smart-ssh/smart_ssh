// @vitest-environment node
//
// Issue #874: `scripts/check-doc-paths.mjs` reports backticked paths and
// quoted section references in the guidance documents that do not exist.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { checkDocPaths } from "../../../../scripts/check-doc-paths.mjs";

const tmp: string[] = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function tree(files: Record<string, string>): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "docpaths-"));
  tmp.push(root);
  for (const [f, content] of Object.entries(files)) {
    const full = path.join(root, f);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, content);
  }
  return root;
}

const FENCE = "```";

describe("check-doc-paths", () => {
  it("passes for the real repository", () => {
    expect(checkDocPaths()).toEqual([]);
  });

  it("fails for a renamed file cited in AGENTS.md", () => {
    const root = tree({ "AGENTS.md": "See `docs/old-name.md`.\n", "docs/new-name.md": "# x\n" });
    const problems = checkDocPaths(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("docs/old-name.md");
  });

  it("passes for an existing path and strips :line and :line-line suffixes", () => {
    const root = tree({ "AGENTS.md": "`src/a.rs:12` and `src/a.rs:12-30` and `src/a.rs`\n", "src/a.rs": "" });
    expect(checkDocPaths(root)).toEqual([]);
  });

  it("does not accept a bare file name that only exists elsewhere in the repository", () => {
    const root = tree({ "AGENTS.md": "`commands.rs`\n", "crates/app-shell/src/commands.rs": "" });
    const problems = checkDocPaths(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("commands.rs");
  });

  it("checks the file even when a line suffix is present", () => {
    const root = tree({ "AGENTS.md": "`src/gone.rs:12-30`\n" });
    expect(checkDocPaths(root)).toHaveLength(1);
  });

  it("resolves relative to the frontend root, src/ and the document directory", () => {
    const root = tree({
      "AGENTS.md": "`e2e/x.ts` `components/y.tsx` `sibling.md`\n",
      "apps/smart-ssh-community/frontend/e2e/x.ts": "",
      "apps/smart-ssh-community/frontend/src/components/y.tsx": "",
    });
    const problems = checkDocPaths(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("sibling.md");
    fs.writeFileSync(path.join(root, "sibling.md"), "");
    expect(checkDocPaths(root)).toEqual([]);
  });

  it.each(["docs/specs/NNNN-slug.md", "locales/<lang>/a.json", "src/*.ts", "src/{a,b}.ts", "v/X.Y.Z/a.md", "a/…/b.rs"])(
    "ignores the placeholder %s",
    (token) => {
      expect(checkDocPaths(tree({ "AGENTS.md": `\`${token}\`\n` }))).toEqual([]);
    },
  );

  it("ignores paths in fenced code blocks", () => {
    const root = tree({ "AGENTS.md": `${FENCE}bash\ncat \`missing/file.md\`\n${FENCE}\n` });
    expect(checkDocPaths(root)).toEqual([]);
  });

  it("fails for a section title that does not exist and passes for one that does", () => {
    const files = { "AGENTS.md": "# Top\n\n## Real section\n" };
    const bad = tree({ ...files, "docs/a.md": 'See `AGENTS.md`, section "Gone".\n' });
    const problems = checkDocPaths(bad);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain('"Gone"');
    expect(checkDocPaths(tree({ ...files, "docs/a.md": 'See `AGENTS.md`, section "real SECTION".\n' }))).toEqual([]);
    expect(checkDocPaths(tree({ ...files, "docs/a.md": "Siehe `AGENTS.md`, Abschnitt „Real section“.\n" }))).toEqual([]);
    expect(checkDocPaths(tree({ ...files, "docs/a.md": "Siehe `AGENTS.md`, „Gone“.\n" }))).toHaveLength(1);
  });

  it("checks AGENTS.md#anchor", () => {
    const files = { "AGENTS.md": "## Real section\n" };
    expect(checkDocPaths(tree({ ...files, "docs/a.md": "`AGENTS.md#real-section`\n" }))).toEqual([]);
    expect(checkDocPaths(tree({ ...files, "docs/a.md": "`AGENTS.md#nope`\n" }))).toHaveLength(1);
  });

  it("accepts an allowlisted token and rejects an entry without a reason", () => {
    const base = { "AGENTS.md": "`target/out.md`\n" };
    const ok = tree({
      ...base,
      "scripts/doc-paths-allowlist.json": JSON.stringify({ "AGENTS.md": [{ token: "target/out.md", reason: "build output" }] }),
    });
    expect(checkDocPaths(ok)).toEqual([]);
    const noReason = tree({
      ...base,
      "scripts/doc-paths-allowlist.json": JSON.stringify({ "AGENTS.md": [{ token: "target/out.md", reason: "" }] }),
    });
    expect(checkDocPaths(noReason).some((p) => p.includes("reason"))).toBe(true);
  });

  it("the real allowlist gives every entry a reason", () => {
    const file = fileURLToPath(new URL("../../../../scripts/doc-paths-allowlist.json", import.meta.url));
    const data = JSON.parse(fs.readFileSync(file, "utf8")) as Record<string, { token: string; reason: string }[]>;
    for (const list of Object.values(data)) {
      for (const e of list) expect(e.reason.trim()).not.toBe("");
    }
  });

  it("--root and --config check a fixture tree", () => {
    const root = tree({
      "handbook/guide.md": "See `lib/a.txt` and `missing.md`.\n",
      "lib/a.txt": "",
      "cfg.json": JSON.stringify({ documents: ["handbook/*.md"], baseDirs: [], allowlist: "al.json" }),
      "al.json": JSON.stringify({ "handbook/guide.md": [{ token: "missing.md", reason: "example" }] }),
    });
    const script = fileURLToPath(new URL("../../../../scripts/check-doc-paths.mjs", import.meta.url));
    const out = execFileSync("node", [script, "--root", root, "--config", "cfg.json"], { encoding: "utf8" });
    expect(out).toContain("Doc paths ok");
    fs.writeFileSync(path.join(root, "al.json"), "{}");
    expect(() => execFileSync("node", [script, "--root", root, "--config", "cfg.json"], { stdio: "pipe" })).toThrow();
  });
});
