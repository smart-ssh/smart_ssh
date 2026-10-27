// @vitest-environment node
//
// Issue #858: `scripts/check-conventions.mjs` rejects new `*_tests.rs` files,
// stale entries in its list of existing ones, and server-specific identifiers
// in the filter/risk code.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { checkConventions } from "../../../../scripts/check-conventions.mjs";

const tmp: string[] = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function tree(files: Record<string, string>): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "conv-"));
  tmp.push(root);
  for (const [f, content] of Object.entries(files)) {
    const full = path.join(root, f);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, content);
  }
  return root;
}

describe("check-conventions", () => {
  it("passes for the real repository", () => {
    expect(checkConventions()).toEqual([]);
  });

  it("fails for a new *_tests.rs file", () => {
    const root = tree({ "crates/a/src/foo_tests.rs": "" });
    const problems = checkConventions(root, []);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("tests_<topic>.rs");
  });

  it("passes for a listed existing *_tests.rs file", () => {
    const f = "crates/a/src/foo_tests.rs";
    expect(checkConventions(tree({ [f]: "" }), [f])).toEqual([]);
  });

  it("accepts tests.rs and tests_<topic>.rs", () => {
    const root = tree({ "crates/a/src/tests.rs": "", "crates/a/src/foo/tests_bar.rs": "" });
    expect(checkConventions(root, [])).toEqual([]);
  });

  it("fails for a listed file that no longer exists", () => {
    const problems = checkConventions(tree({ "crates/a/src/lib.rs": "" }), ["crates/a/src/gone_tests.rs"]);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("remove it from the list");
  });

  it.each([
    ["crates/core/src/filter/engine.rs", "LOCAL_SERVER_ID"],
    ["crates/core/src/filter/tests.rs", "local_server"],
    ["crates/core/src/risk/nested/x.rs", "if id == LOCAL_SERVER_ID {}"],
  ])("fails for a server-specific identifier in %s", (file, content) => {
    const problems = checkConventions(tree({ [file]: content }), []);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("No special-casing");
  });

  it("allows the identifier outside filter/risk", () => {
    const root = tree({ "crates/app-shell/src/local_server.rs": "LOCAL_SERVER_ID" });
    expect(checkConventions(root, [])).toEqual([]);
  });
});
