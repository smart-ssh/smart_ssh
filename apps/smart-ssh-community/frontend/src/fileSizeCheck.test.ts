// @vitest-environment node
//
// Issue #841: `scripts/check-file-size.mjs` keeps source files from growing
// past their recorded size. Cases run on temporary fixture git repositories.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { isTestFile, limitFor, main } from "../../../../scripts/check-file-size.mjs";

const REPO_ROOT = path.resolve(__dirname, "../../../..");
const tmpDirs: string[] = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function git(cwd: string, ...args: string[]) {
  return execFileSync(
    "git",
    ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "protocol.file.allow=always", ...args],
    { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
  );
}

function lines(n: number): string {
  return "// x\n".repeat(n);
}

function fixture(files: Record<string, number>, allowlist: Record<string, number>): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "file-size-"));
  tmpDirs.push(dir);
  git(dir, "init", "-q");
  for (const [rel, n] of Object.entries(files)) write(dir, rel, n);
  writeAllowlist(dir, allowlist);
  git(dir, "add", ".");
  git(dir, "commit", "-qm", "init");
  return dir;
}

function write(dir: string, rel: string, n: number) {
  fs.mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
  fs.writeFileSync(path.join(dir, rel), lines(n));
}

const ALLOW = "scripts/file-size-allowlist.json";
function writeAllowlist(dir: string, list: Record<string, number>) {
  fs.mkdirSync(path.join(dir, "scripts"), { recursive: true });
  fs.writeFileSync(path.join(dir, ALLOW), JSON.stringify(list, null, 2) + "\n");
}

function run(dir: string, ...extra: string[]) {
  const logs: string[] = [];
  const out = { log: (m: string) => logs.push(m), error: (m: string) => logs.push(m) };
  const code = main(["--root", dir, "--allowlist", path.join(dir, ALLOW), ...extra], out);
  return { code, text: logs.join("\n") };
}

describe("check-file-size", () => {
  it("passes on the real repository", () => {
    const logs: string[] = [];
    const out = { log: (m: string) => logs.push(m), error: (m: string) => logs.push(m) };
    expect(main(["--root", REPO_ROOT], out), logs.join("\n")).toBe(0);
  });

  it("classifies files", () => {
    expect(isTestFile("crates/app-logic/src/test_connection.rs")).toBe(false);
    expect(isTestFile("crates/core/src/filter/tests.rs")).toBe(true);
    expect(isTestFile("crates/x/tests/integration.rs")).toBe(true);
    expect(isTestFile("crates/x/src/a_tests.rs")).toBe(true);
    expect(isTestFile("crates/x/src/tests_a.rs")).toBe(true);
    expect(isTestFile("f/src/A.test.tsx")).toBe(true);
    expect(isTestFile("f/e2e/tests/a.ts")).toBe(true);
    expect(limitFor("crates/x/src/lib.rs")).toBe(1200);
    expect(limitFor("crates/core/src/filter/tests.rs")).toBe(2000);
  });

  it("fails when an allowlisted file grows past its entry", () => {
    const dir = fixture({ "a.rs": 1300 }, { "a.rs": 1300 });
    expect(run(dir).code).toBe(0);
    write(dir, "a.rs", 1301);
    const r = run(dir);
    expect(r.code).toBe(1);
    expect(r.text).toContain("a.rs: 1301 lines, allowlist entry 1300");
    expect(r.text).toContain("split the file");
  });

  it("fails for a new production file with 1,201 lines, passes at 1,200", () => {
    const dir = fixture({ "ok.rs": 1200, "bad.rs": 1201 }, {});
    const r = run(dir);
    expect(r.code).toBe(1);
    expect(r.text).toContain("bad.rs");
    expect(r.text).not.toContain("ok.rs");
  });

  it("passes for a new test file with 2,000 lines, fails at 2,001", () => {
    expect(run(fixture({ "src/a.test.ts": 2000 }, {})).code).toBe(0);
    expect(run(fixture({ "src/a.test.ts": 2001 }, {})).code).toBe(1);
  });

  it("--update lowers entries but never raises or adds", () => {
    const dir = fixture(
      { "a.rs": 1500, "b.rs": 1300, "c.rs": 1400, "d.rs": 100, "new.rs": 1300 },
      { "a.rs": 1600, "b.rs": 1200 + 50, "c.rs": 1400, "d.rs": 1300, "gone.rs": 1300 },
    );
    const r = run(dir, "--update");
    expect(r.code).toBe(1); // b.rs (1300 > 1250) and new.rs still violate
    const list = JSON.parse(fs.readFileSync(path.join(dir, ALLOW), "utf8"));
    expect(list).toEqual({ "a.rs": 1500, "b.rs": 1250, "c.rs": 1400 });
  });

  it("fails on stale entries and names --update", () => {
    const dir = fixture({ "small.rs": 10 }, { "small.rs": 1300, "gone.rs": 1300 });
    const r = run(dir);
    expect(r.code).toBe(1);
    expect(r.text).toContain("small.rs");
    expect(r.text).toContain("gone.rs");
    expect(r.text).toContain("stale entry, run --update");
    expect(run(dir, "--update").code).toBe(0);
  });

  it("--base fails on a raised or added entry", () => {
    const dir = fixture({ "a.rs": 1300, "b.rs": 1300 }, { "a.rs": 1300 });
    const base = git(dir, "rev-parse", "HEAD").trim();
    expect(run(dir, "--base", base).code).toBe(1); // b.rs has no entry at all
    writeAllowlist(dir, { "a.rs": 1300, "b.rs": 1300 });
    let r = run(dir, "--base", base);
    expect(r.code).toBe(1);
    expect(r.text).toContain("new allowlist entry");
    write(dir, "a.rs", 1400);
    writeAllowlist(dir, { "a.rs": 1400 });
    write(dir, "b.rs", 10);
    r = run(dir, "--base", base);
    expect(r.code).toBe(1);
    expect(r.text).toContain("raised from 1300 to 1400");
    write(dir, "a.rs", 1200);
    writeAllowlist(dir, {});
    write(dir, "b.rs", 10);
    expect(run(dir, "--base", base).code).toBe(0);
  });

  it("works on a tree outside the repository with --root/--allowlist", () => {
    const dir = fixture({ "x/y.ts": 1300 }, {});
    const list = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "alw-")), "list.json");
    tmpDirs.push(path.dirname(list));
    fs.writeFileSync(list, JSON.stringify({ "x/y.ts": 1300 }));
    const out = { log: () => {}, error: () => {} };
    expect(main(["--root", dir, "--allowlist", list], out)).toBe(0);
  });

  it("does not count files inside a git submodule", () => {
    const sub = fixture({ "big.rs": 5000 }, {});
    const dir = fixture({ "a.rs": 10 }, {});
    git(dir, "submodule", "add", "-q", sub, "vendor/sub");
    git(dir, "commit", "-qm", "sub");
    expect(fs.existsSync(path.join(dir, "vendor/sub/big.rs"))).toBe(true);
    const r = run(dir);
    expect(r.code, r.text).toBe(0);
    expect(r.text).toContain("ok");
  });
});
