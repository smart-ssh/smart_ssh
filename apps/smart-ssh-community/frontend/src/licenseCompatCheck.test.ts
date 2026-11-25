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

    it("rejects headers with a trailing comment or whitespace", () => {
      for (const h of ["[[licenses.exceptions]] # note", "[[licenses.exceptions]]  "]) {
        expect(() => parseExceptions(raw(`${h}\ncrate = "x"\n`))).toThrow(/array of tables/);
      }
    });

    it("rejects a CRLF header with a trailing comment", () => {
      const body = '[[licenses.exceptions]] # note\r\ncrate = "x"\r\nallow = ["GPL-2.0"]\r\n';
      expect(() => parseExceptions(raw(body))).toThrow(/array of tables/);
      const bad = spawnSync(process.execPath, [SCRIPT, raw(body)], { encoding: "utf8" });
      expect(bad.status).not.toBe(0);
      expect(bad.stderr).toMatch(/array of tables/);
    }, 30_000);

    it("rejects the dotted-key form", () => {
      const body = 'licenses.exceptions = [{ crate = "x", allow = ["GPL-2.0"] }]\n';
      expect(() => parseExceptions(raw(body))).toThrow(/dotted key/);
    });

    it("ignores a commented-out array-of-tables header", () => {
      expect(parseExceptions(raw('# [[licenses.exceptions]]\n#licenses.exceptions = []\n'))).toEqual([]);
    });

    const FORMS: [string, string, RegExp][] = [
      ["array of tables", '[[licenses.exceptions]]\ncrate = "x"\nallow = ["GPL-2.0"]\n', /array of tables/],
      ["dotted key", 'licenses.exceptions = [{ crate = "x", allow = ["GPL-2.0"] }]\n', /dotted key/],
      ["quoted array of tables", '[[licenses."exceptions"]]\ncrate = "x"\n', /array of tables/],
      ["quoted licenses", '"licenses".exceptions = [{ crate = "x", allow = ["GPL-2.0"] }]\n', /quoted key/],
      ["quoted exceptions", "licenses.'exceptions' = [{ crate = \"x\", allow = [\"GPL-2.0\"] }]\n", /quoted key/],
      ["spaced dots", 'licenses . "exceptions" = [{ crate = "x", allow = ["GPL-2.0"] }]\n', /quoted key/],
      ["inline table", 'licenses = { exceptions = [{ crate = "x", allow = ["GPL-2.0"] }] }\n', /inline table/],
      ["escaped exceptions key", 'licenses."exc\\u0065ptions" = [{ crate = "x", allow = ["GPL-2.0"] }]\n', /quoted key with escape sequence/],
      ["escaped exceptions key with quoted licenses", '"licenses"."exc\\u0065ptions" = [{ crate = "x", allow = ["GPL-2.0"] }]\n', /quoted key with escape sequence/],
      ["escaped array of tables", '[[licenses."exc\\u0065ptions"]]\ncrate = "x"\n', /quoted key with escape sequence/],
      ["escaped key in inline table", 'licenses = { "exc\\u0065ptions" = [{ crate = "x", allow = ["GPL-2.0"] }] }\n', /quoted key with escape sequence/],
    ];

    it.each(FORMS)("rejects %s, also with comment and CRLF", (_n, body, re) => {
      expect(() => parseExceptions(raw(body))).toThrow(re);
      const withComment = body.replace(/\n/, " # note\n");
      expect(() => parseExceptions(raw(withComment))).toThrow(re);
      expect(() => parseExceptions(raw(withComment.replace(/\n/g, "\r\n")))).toThrow(re);
    });

    it("does not throw for commented-out forms", () => {
      for (const [, body] of FORMS) {
        expect(parseExceptions(raw(body.replace(/^/gm, "# ")))).toEqual([]);
      }
    });

    it.each(FORMS)(
      "check-license-compat exits non-zero naming the form: %s",
      (_n, body, re) => {
        const bad = spawnSync(process.execPath, [SCRIPT, raw(body)], { encoding: "utf8" });
        expect(bad.status).not.toBe(0);
        expect(bad.stderr).toMatch(re);
      },
      30_000,
    );

    it("accepts a value string containing a backslash", () => {
      const body = 'exceptions = [\n    { crate = "x", allow = ["MIT"] },\n]\nnote = "a\\\\b \\u0041"\n';
      expect(parseExceptions(raw(body))).toEqual([{ crate: "x", allow: ["MIT"] }]);
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
    }, 30_000);
  });
});
