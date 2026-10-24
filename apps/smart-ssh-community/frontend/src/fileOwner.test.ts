import { describe, expect, it } from "vitest";
import { formatOwner } from "./fileOwner";

describe("formatOwner", () => {
  it("shows user:group when names are known", () => {
    expect(formatOwner({ owner: "alice", group: "staff", uid: 1, gid: 2 })).toBe("alice:staff");
  });
  it("falls back to uid:gid when names are missing", () => {
    expect(formatOwner({ owner: null, group: null, uid: 1000, gid: 100 })).toBe("1000:100");
  });
  it("mixes a name with a number per side", () => {
    expect(formatOwner({ owner: "root", group: null, uid: 0, gid: 7 })).toBe("root:7");
  });
  it("shows a dash when nothing is known", () => {
    expect(formatOwner({ owner: null, group: null, uid: null, gid: null })).toBe("—");
  });
});
