// Issue #49: welche Gruppe ein neuer Server / eine neue Gruppe in der
// Verwalten-Sidebar als Vorgabe bekommt.
import { describe, expect, it } from "vitest";
import { folderForNewItem } from "./newItemFolder";
import type { ServerDto } from "./types";

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

const servers = [server("local", null, true), server("free", null), server("web-1", "g-web")];

describe("folderForNewItem (issue #49)", () => {
  it("uses no folder when nothing is selected", () => {
    expect(folderForNewItem(null, servers)).toBeNull();
  });

  it("uses the selected folder", () => {
    expect(folderForNewItem({ kind: "group", id: "g-web" }, servers)).toBe("g-web");
  });

  it("uses the folder of the selected server", () => {
    expect(folderForNewItem({ kind: "server", id: "web-1" }, servers)).toBe("g-web");
  });

  it("uses no folder for an ungrouped, the local or an unknown server", () => {
    expect(folderForNewItem({ kind: "server", id: "free" }, servers)).toBeNull();
    expect(folderForNewItem({ kind: "server", id: "local" }, servers)).toBeNull();
    expect(folderForNewItem({ kind: "server", id: "gone" }, servers)).toBeNull();
  });

  it("keeps the folder of an open new-item form", () => {
    expect(folderForNewItem({ kind: "newServer", groupId: "g-web" }, servers)).toBe("g-web");
    expect(folderForNewItem({ kind: "newGroup", parentId: "g-prod" }, servers)).toBe("g-prod");
    expect(folderForNewItem({ kind: "newServer", groupId: null }, servers)).toBeNull();
  });
});
