// @vitest-environment node
//
// Issue #867: `scripts/check-codemap.mjs` keeps every file and symbol named in
// `docs/codemap.md` real.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { checkCodemap, findCodemapProblems } from "../../../../scripts/check-codemap.mjs";

let root: string;
beforeAll(() => {
  root = fs.mkdtempSync(path.join(os.tmpdir(), "codemap-"));
  fs.mkdirSync(path.join(root, "crates", "x", "src"), { recursive: true });
  fs.writeFileSync(path.join(root, "crates/x/src/lib.rs"), "pub async fn run_it() {}\npub struct Thing;\n");
  fs.writeFileSync(path.join(root, "crates/x/src/ui.tsx"), "export const Panel = () => null;\nexport function helper() {}\n");
});
afterAll(() => fs.rmSync(root, { recursive: true, force: true }));

const messages = (md: string) => findCodemapProblems(md, root).map((p) => p.message);

describe("check-codemap", () => {
  it("passes for the real docs/codemap.md", () => {
    expect(checkCodemap()).toEqual([]);
  });

  it("accepts existing paths and symbols", () => {
    expect(messages("| `crates/x/src/lib.rs` — `run_it()`, `Thing` | a |\n- `crates/x/src/ui.tsx` — `Panel`, `helper`\n")).toEqual([]);
  });

  it("fails on a missing file", () => {
    expect(messages("`crates/x/src/gone.rs` — `run_it`")).toEqual(["path does not exist: crates/x/src/gone.rs"]);
  });

  it("fails on a renamed symbol", () => {
    expect(messages("`crates/x/src/lib.rs` — `run_that`")).toEqual(["no definition of `run_that` in crates/x/src/lib.rs"]);
    expect(messages("`crates/x/src/ui.tsx` — `Panel2`")).toHaveLength(1);
  });

  it("ignores paths and symbols in fenced blocks", () => {
    expect(messages("```bash\ngrep `crates/x/src/gone.rs` `nope`\n```\n")).toEqual([]);
  });

  it("does not attach a symbol of one list item to the path of the previous one", () => {
    expect(messages("- `crates/x/src/lib.rs` — `Thing`\n- prose `Nope`\n")).toEqual([]);
  });
});
