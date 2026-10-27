// @vitest-environment node
//
// Issue #928: the core defines no list of tiers. A `Tier` enum (Rust) or a
// string-literal union for tiers (TypeScript) must not come back.
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const REPO_ROOT = path.resolve(__dirname, "../../../..");
const read = (p: string) => fs.readFileSync(path.join(REPO_ROOT, p), "utf8");

describe("no fixed tier list in the core", () => {
  it("Rust: no `enum Tier`", () => {
    const rs = read("crates/core/src/entitlements.rs");
    expect(rs).not.toMatch(/\benum\s+Tier\b/);
    expect(rs).toMatch(/pub struct TierId/);
  });

  it("TypeScript: Tier is a plain string, not a union", () => {
    const ts = read("apps/smart-ssh-community/frontend/src/extensions/entitlements.ts");
    expect(ts).toMatch(/export type Tier = string;/);
    expect(ts).not.toMatch(/type\s+Tier\s*=\s*"/);
  });
});
