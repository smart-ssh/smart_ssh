// @vitest-environment node
//
// Issue #1583: `scripts/check-license-compat.mjs` keeps every license allowed
// in `deny.toml` GPL-3.0-compatible.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { checkLicenseCompat } from "../../../../scripts/check-license-compat.mjs";

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
});
