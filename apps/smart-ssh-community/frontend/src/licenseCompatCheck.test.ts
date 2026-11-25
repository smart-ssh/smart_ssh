// @vitest-environment node
//
// Issue #1583: `scripts/check-license-compat.mjs` keeps every license allowed
// in `deny.toml` GPL-3.0-compatible.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { checkLicenseCompat } from "../../../../scripts/check-license-compat.mjs";
import { parseExceptions } from "../../../../scripts/generate-third-party-notices.mjs";

const tmp: string[] = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function deny(allow: string[], exceptions: string[] = []): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "licompat-"));
  tmp.push(dir);
  const file = path.join(dir, "deny.toml");
  const list = (a: string[]) => a.map((x) => `"${x}"`).join(", ");
  const exc = exceptions.length
    ? `exceptions = [\n    { crate = "foo", allow = [${list(exceptions)}] },\n]\n`
    : "";
  fs.writeFileSync(file, `[licenses]\nallow = [${list(allow)}]\n${exc}`);
  return file;
}

describe("check-license-compat", () => {
  it("passes on the current deny.toml", () => {
    expect(checkLicenseCompat()).toEqual([]);
  });

  it("fails and names GPL-2.0-only in allow", () => {
    const problems = checkLicenseCompat(deny(["MIT", "GPL-2.0-only"]));
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("GPL-2.0-only");
  });

  it("fails for OpenSSL in an exceptions entry", () => {
    const problems = checkLicenseCompat(deny(["MIT"], ["OpenSSL"]));
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("OpenSSL");
    expect(problems[0]).toContain("foo");
  });

  it("fails for a LicenseRef-* entry", () => {
    const problems = checkLicenseCompat(deny(["MIT", "LicenseRef-Custom"]));
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("LicenseRef-Custom");
  });

  it.each(["LGPL-2.1-only", "EPL-1.0", "EPL-2.0", "CDDL-1.0", "CDDL-1.1", "SSPL-1.0", "BUSL-1.1", "SSLeay", "Commons-Clause", "CC-BY-NC-4.0"])(
    "fails for %s",
    (id) => {
      expect(checkLicenseCompat(deny([id]))).toHaveLength(1);
    },
  );

  describe("parseExceptions", () => {
    const raw = (body: string): string => {
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "licompat-"));
      tmp.push(dir);
      const file = path.join(dir, "deny.toml");
      fs.writeFileSync(file, `[licenses]\nallow = ["MIT"]\n${body}`);
      return file;
    };
    const SCRIPT = path.resolve(__dirname, "../../../../scripts/check-license-compat.mjs");

    it("throws for an entry with allow before crate", () => {
      expect(() => parseExceptions(raw('exceptions = [\n    { allow = ["MIT"], crate = "x" },\n]\n'))).toThrow(/allow = \["MIT"\], crate = "x"/);
    });

    it("throws for an entry with an extra version key", () => {
      expect(() =>
        parseExceptions(raw('exceptions = [\n    { crate = "x", version = "1", allow = ["MIT"] },\n]\n')),
      ).toThrow(/version/);
    });

    it("throws for an empty allow list", () => {
      expect(() => parseExceptions(raw('exceptions = [\n    { crate = "x", allow = [] },\n]\n'))).toThrow(/x/);
    });

    it("returns [] without a block and for an empty block", () => {
      expect(parseExceptions(raw(""))).toEqual([]);
      expect(parseExceptions(raw("exceptions = []\n"))).toEqual([]);
      expect(parseExceptions(raw("exceptions = [\n    # nothing\n]\n"))).toEqual([]);
    });

    it("reads single-line and indented exceptions blocks", () => {
      expect(parseExceptions(raw('exceptions = [{ crate = "x", allow = ["GPL-2.0"] }]\n'))).toEqual([
        { crate: "x", allow: ["GPL-2.0"] },
      ]);
      expect(
        parseExceptions(raw('  exceptions = [\n    { crate = "y", allow = ["MIT"] },\n  ]\n')),
      ).toEqual([{ crate: "y", allow: ["MIT"] }]);
    });

    it("fails when the exceptions block has no closing bracket", () => {
      expect(() => parseExceptions(raw('exceptions = [{ crate = "x", allow = ["MIT"] }\n'))).toThrow();
    });

    it("ignores commented-out entries", () => {
      expect(
        parseExceptions(raw('exceptions = [\n    # { crate = "x", allow = ["GPL-2.0"] },\n]\n')),
      ).toEqual([]);
    });

    it("rejects the array-of-tables form, naming the form", () => {
      const body = '[[licenses.exceptions]]\ncrate = "x"\nallow = ["GPL-2.0"]\n';
      expect(() => parseExceptions(raw(body))).toThrow(/array of tables/);
    });

    it("rejects the dotted-key form", () => {
      const body = 'licenses.exceptions = [{ crate = "x", allow = ["GPL-2.0"] }]\n';
      expect(() => parseExceptions(raw(body))).toThrow(/dotted key/);
    });

    it("ignores a commented-out array-of-tables header", () => {
      expect(parseExceptions(raw('# [[licenses.exceptions]]\n#licenses.exceptions = []\n'))).toEqual([]);
    });

    it("check-license-compat exits non-zero for both alternative forms", () => {
      for (const body of [
        '[[licenses.exceptions]]\ncrate = "x"\nallow = ["GPL-2.0"]\n',
        'licenses.exceptions = [{ crate = "x", allow = ["GPL-2.0"] }]\n',
      ]) {
        const bad = spawnSync(process.execPath, [SCRIPT, raw(body)], { encoding: "utf8" });
        expect(bad.status).not.toBe(0);
        expect(bad.stderr).toContain("exceptions");
      }
    });

    it("returns the sqlcipher-src entry of the repo deny.toml", () => {
      expect(parseExceptions(path.resolve(__dirname, "../../../../deny.toml"))).toEqual([
        { crate: "sqlcipher-src", allow: ["blessing", "WTFPL"] },
      ]);
    });

    it("check-license-compat exits non-zero with the message for an unparseable entry", () => {
      const file = raw('exceptions = [\n    { allow = ["MIT"], crate = "x" },\n]\n');
      expect(() => checkLicenseCompat(file)).toThrow(/crate = "x"/);
      const bad = spawnSync(process.execPath, [SCRIPT, file], { encoding: "utf8" });
      expect(bad.status).not.toBe(0);
      expect(bad.stderr).toContain('crate = "x"');
      const ok = spawnSync(process.execPath, [SCRIPT], { encoding: "utf8" });
      expect(ok.status).toBe(0);
    });
  });
});
