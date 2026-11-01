import { describe, expect, it } from "vitest";
import type { ChatItem } from "./components/ChatPanel";
import { mergeResyncedItems } from "./chatResync";

const error: ChatItem = {
  type: "error",
  id: "e1",
  message: "send failed",
  code: null,
};
const user: ChatItem = { type: "user", id: "u1", text: "hi" };
const rebuiltUser: ChatItem = { type: "user", id: "u2", text: "hi" };

describe("mergeResyncedItems live-only items (issue #1041)", () => {
  it("keeps an error card after the re-sync", () => {
    const merged = mergeResyncedItems([user, error], [rebuiltUser], null);
    expect(merged.map((i) => i.type)).toEqual(["user", "error"]);
  });

  it("shows the error card exactly once after two re-syncs", () => {
    const first = mergeResyncedItems([user, error], [rebuiltUser], null);
    const second = mergeResyncedItems(
      first,
      [{ ...rebuiltUser, id: "u3" }],
      null,
    );
    expect(second.filter((i) => i.type === "error")).toHaveLength(1);
    expect(second.filter((i) => i.type === "user")).toHaveLength(1);
  });

  it("does not duplicate history items", () => {
    const merged = mergeResyncedItems([user], [rebuiltUser], null);
    expect(merged).toEqual([rebuiltUser]);
  });
});
