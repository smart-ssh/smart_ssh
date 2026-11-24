// @vitest-environment node
//
// Issue #897: `scripts/check-workspace-deps.mjs` requires shared third-party
// crates to be declared once in the root `[workspace.dependencies]`.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { checkWorkspaceDeps } from "../../../../scripts/check-workspace-deps.mjs";

const tmp: string[] = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function tree(files: Record<string, string>): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "wsdeps-"));
  tmp.push(root);
  for (const [f, content] of Object.entries(files)) {
    const full = path.join(root, f);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, content);
  }
  return root;
}

const ROOT = '[workspace]\nmembers = ["crates/*"]\n\n[workspace.dependencies]\nserde = { version = "1" }\n';

describe("check-workspace-deps", () => {
  it("passes for the real repository", () => {
    expect(checkWorkspaceDeps()).toEqual([]);
  });

  it("fails when a crate pins a version of a workspace dependency", () => {
    const root = tree({
      "Cargo.toml": ROOT,
      "crates/a/Cargo.toml": '[dependencies]\nserde = "1"\n',
    });
    const problems = checkWorkspaceDeps(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("workspace = true");
  });

  it("fails when two crates share a crate missing from the workspace table", () => {
    const root = tree({
      "Cargo.toml": ROOT,
      "crates/a/Cargo.toml": '[dependencies]\nserde = { workspace = true }\nrand = "0.9"\n',
      "crates/b/Cargo.toml": '[dev-dependencies]\nrand = { version = "0.9" }\n',
    });
    const problems = checkWorkspaceDeps(root);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("`rand`");
  });

  it("accepts inherited entries and single-use pins", () => {
    const root = tree({
      "Cargo.toml": ROOT,
      "crates/a/Cargo.toml":
        '[dependencies]\nserde = { workspace = true, features = ["derive"] }\nonly-here = "1"\n' +
        '[target.\'cfg(unix)\'.dependencies]\nlibc = "0.2"\n',
      "crates/b/Cargo.toml": '[dependencies]\nserde = { workspace = true }\nlocal = { path = "../a" }\n',
    });
    expect(checkWorkspaceDeps(root)).toEqual([]);
  });
});
