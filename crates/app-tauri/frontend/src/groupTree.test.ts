import { describe, expect, it } from "vitest";
import { buildGroupTree } from "./groupTree";
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
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    isLocal: false,
    postIngestPolicy: "balanced",
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
