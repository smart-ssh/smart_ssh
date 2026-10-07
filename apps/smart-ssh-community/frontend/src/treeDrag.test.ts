import { afterEach, describe, expect, it, vi } from "vitest";
import {
  classifyDrop,
  groupDropTargetValue,
  parseDropTarget,
  performMove,
  type DragItem,
} from "./treeDrag";
import { moveGroup, moveServerToGroup } from "./api";
import type { GroupDto, ServerDto } from "./types";

// Issue #48 / Spec 0103: Regeln für das Verschieben per Drag-and-drop.

vi.mock("./api", () => ({
  moveGroup: vi.fn(() => Promise.resolve()),
  moveServerToGroup: vi.fn(() => Promise.resolve()),
}));

function group(id: string, parentId: string | null): GroupDto {
  return { id, name: id, parentId, notes: "" };
}

function server(id: string, groupId: string | null, isLocal = false): ServerDto {
  return {
    id,
    name: id,
    host: "example.invalid",
    port: 22,
    username: "user",
    groupId,
    tags: [],
    authKind: "agent",
    identityFilePath: null,
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    sudoPasswordUnknown: false,
    isLocal,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
    sftpServerPath: null,
  };
}

// a ─┬─ b ── c
//    └─ (srv-a)
// d
const groups = [group("a", null), group("b", "a"), group("c", "b"), group("d", null)];
const servers = [server("local", null, true), server("srv-a", "a"), server("srv-root", null)];

const srv = (id: string): DragItem => ({ kind: "server", id, label: id });
const grp = (id: string): DragItem => ({ kind: "group", id, label: id });

describe("parseDropTarget", () => {
  it("parses root, group and rejects everything else", () => {
    expect(parseDropTarget("root")).toEqual({ kind: "root" });
    expect(parseDropTarget(groupDropTargetValue("g1"))).toEqual({ kind: "group", id: "g1" });
    expect(parseDropTarget("none")).toBeNull();
    expect(parseDropTarget("group:")).toBeNull();
    expect(parseDropTarget(null)).toBeNull();
    expect(parseDropTarget(undefined)).toBeNull();
  });
});

describe("classifyDrop for servers", () => {
  it("moves a server into a group, a subgroup and out of any group", () => {
    expect(classifyDrop(srv("srv-root"), { kind: "group", id: "a" }, groups, servers)).toBe("valid");
    expect(classifyDrop(srv("srv-a"), { kind: "group", id: "c" }, groups, servers)).toBe("valid");
    expect(classifyDrop(srv("srv-a"), { kind: "root" }, groups, servers)).toBe("valid");
  });

  it("is a no-op where the server already is", () => {
    expect(classifyDrop(srv("srv-a"), { kind: "group", id: "a" }, groups, servers)).toBe("noop");
    expect(classifyDrop(srv("srv-root"), { kind: "root" }, groups, servers)).toBe("noop");
  });

  it("never moves the local pseudo-server", () => {
    expect(classifyDrop(srv("local"), { kind: "group", id: "a" }, groups, servers)).toBe("noop");
  });
});

describe("classifyDrop for groups", () => {
  it("nests a group into another group and moves it back to the top level", () => {
    expect(classifyDrop(grp("d"), { kind: "group", id: "c" }, groups, servers)).toBe("valid");
    expect(classifyDrop(grp("b"), { kind: "root" }, groups, servers)).toBe("valid");
    expect(classifyDrop(grp("c"), { kind: "group", id: "a" }, groups, servers)).toBe("valid");
  });

  it("flags a move into the group's own descendant as a cycle", () => {
    expect(classifyDrop(grp("a"), { kind: "group", id: "b" }, groups, servers)).toBe("cycle");
    expect(classifyDrop(grp("a"), { kind: "group", id: "c" }, groups, servers)).toBe("cycle");
    expect(classifyDrop(grp("b"), { kind: "group", id: "c" }, groups, servers)).toBe("cycle");
  });

  it("is a no-op onto itself, its current parent, or root when already top level", () => {
    expect(classifyDrop(grp("b"), { kind: "group", id: "b" }, groups, servers)).toBe("noop");
    expect(classifyDrop(grp("b"), { kind: "group", id: "a" }, groups, servers)).toBe("noop");
    expect(classifyDrop(grp("a"), { kind: "root" }, groups, servers)).toBe("noop");
  });

  it("terminates on corrupt cyclic data instead of looping forever", () => {
    const cyclic = [group("x", "y"), group("y", "x"), group("z", null)];
    expect(classifyDrop(grp("z"), { kind: "group", id: "x" }, cyclic, [])).toBe("valid");
  });
});

describe("performMove", () => {
  afterEach(() => vi.clearAllMocks());

  it("uses the narrow move commands with null for the top level", async () => {
    await performMove(srv("srv-a"), { kind: "root" });
    expect(moveServerToGroup).toHaveBeenCalledWith("srv-a", null);
    await performMove(srv("srv-root"), { kind: "group", id: "b" });
    expect(moveServerToGroup).toHaveBeenCalledWith("srv-root", "b");
    await performMove(grp("d"), { kind: "group", id: "a" });
    expect(moveGroup).toHaveBeenCalledWith("d", "a");
    await performMove(grp("b"), { kind: "root" });
    expect(moveGroup).toHaveBeenCalledWith("b", null);
  });
});
