import { describe, expect, it } from "vitest";
import { buildGroupTree, flattenGroupOptions, groupOptionLabel } from "./groupTree";
import type { GroupDto, ServerDto } from "./types";

// Spec 0033, Abschnitt 3/4/5 — reine Baum-Aufbau-Logik, geteilt zwischen
// Sidebar (Verwalten-Tab) und der gruppierten Hauptübersicht.

function group(id: string, name: string, parentId: string | null): GroupDto {
  return { id, name, parentId, notes: "" };
}

function server(
  id: string,
  name: string,
  groupId: string | null,
  overrides: Partial<ServerDto> = {},
): ServerDto {
  return {
    id,
    name,
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
    isLocal: false,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
    sftpServerPath: null,
    ...overrides,
  };
}

describe("buildGroupTree", () => {
  it("ordnet Server ihrer direkten Gruppe zu und baut Untergruppen verschachtelt", () => {
    const groups = [group("g1", "Prod", null), group("g1a", "Prod/Web", "g1")];
    const servers = [server("s1", "web-1", "g1a"), server("s2", "db-1", "g1")];

    const tree = buildGroupTree(groups, servers);

    expect(tree.roots).toHaveLength(1);
    expect(tree.roots[0].group.id).toBe("g1");
    expect(tree.roots[0].servers.map((s) => s.id)).toEqual(["s2"]);
    expect(tree.roots[0].children).toHaveLength(1);
    expect(tree.roots[0].children[0].group.id).toBe("g1a");
    expect(tree.roots[0].children[0].servers.map((s) => s.id)).toEqual(["s1"]);
  });

  it("sammelt ungruppierte Server separat, unter Ausschluss des lokalen Pseudo-Servers", () => {
    const servers = [
      server("s1", "loose", null),
      server("local", "Localhost", null, { isLocal: true }),
    ];

    const tree = buildGroupTree([], servers);

    expect(tree.ungroupedServers.map((s) => s.id)).toEqual(["s1"]);
  });

  it("zeigt eine leere Gruppe weiterhin, wenn eine Untergruppe Server enthält", () => {
    const groups = [group("empty", "Leer", null), group("child", "Kind", "empty")];
    const servers = [server("s1", "srv", "child")];

    const tree = buildGroupTree(groups, servers);

    expect(tree.roots).toHaveLength(1);
    expect(tree.roots[0].group.id).toBe("empty");
    expect(tree.roots[0].servers).toEqual([]);
    expect(tree.roots[0].children).toHaveLength(1);
    expect(tree.roots[0].children[0].servers.map((s) => s.id)).toEqual(["s1"]);
  });

  it("liefert eine leere Struktur ohne Gruppen/Server", () => {
    const tree = buildGroupTree([], []);
    expect(tree.roots).toEqual([]);
    expect(tree.ungroupedServers).toEqual([]);
  });
});

describe("flattenGroupOptions (issue #49)", () => {
  const g = (id: string, name: string, parentId: string | null): GroupDto => ({
    id,
    name,
    parentId,
    notes: "",
  });

  it("lists groups in tree order with depth and full path", () => {
    const groups = [
      g("web", "Web", "prod"),
      g("prod", "Prod", null),
      g("db", "DB", "prod"),
      g("stage", "Stage", null),
      g("web-stage", "Web", "stage"),
      g("edge", "Edge", "web"),
    ];
    const options = flattenGroupOptions(groups);
    expect(options.map((o) => [o.group.id, o.depth, o.path])).toEqual([
      ["prod", 0, "Prod"],
      ["web", 1, "Prod / Web"],
      ["edge", 2, "Prod / Web / Edge"],
      ["db", 1, "Prod / DB"],
      ["stage", 0, "Stage"],
      ["web-stage", 1, "Stage / Web"],
    ]);
  });

  it("indents the label by depth with non-breaking spaces", () => {
    const [root, child] = flattenGroupOptions([g("prod", "Prod", null), g("web", "Web", "prod")]);
    expect(groupOptionLabel(root)).toBe("Prod");
    expect(groupOptionLabel(child)).toBe("  Prod / Web");
  });

  it("never drops a group with a missing parent or in a cycle", () => {
    const groups = [g("orphan", "Orphan", "missing"), g("a", "A", "b"), g("b", "B", "a")];
    const ids = flattenGroupOptions(groups).map((o) => o.group.id);
    expect(ids.sort()).toEqual(["a", "b", "orphan"]);
  });
});
