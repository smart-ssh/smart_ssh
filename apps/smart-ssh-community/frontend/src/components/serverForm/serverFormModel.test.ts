import { describe, expect, it } from "vitest";
import type { ServerDto } from "../../api/types";
import {
  authStateFromKind,
  initialServerFormState,
  serverFormReducer,
  toAuthMethodInput,
} from "./serverFormModel";

const server: ServerDto = { isCopy: false,
  id: "s1",
  name: "Prod",
  host: "prod.example.com",
  port: 2222,
  username: "root",
  groupId: "g1",
  tags: ["a", "b"],
  authKind: "password",
  identityFilePath: null,
  jumpHost: "j1",
  notes: "some notes",
  hasSudoPassword: true,
  sudoPasswordUnknown: true,
  isLocal: false,
  postIngestPolicy: "strict",
  aiInjectionCheckEnabled: true,
  sftpServerPath: "/usr/lib/sftp-server",
  startDirectory: "~/work",
};

const fresh = () => initialServerFormState("g0");

describe("serverFormReducer", () => {
  it("load fills all fields and clears dirty", () => {
    const dirty = serverFormReducer(fresh(), { type: "markDirty" });
    const s = serverFormReducer(dirty, { type: "load", server });
    expect(s).toMatchObject({
      loaded: server,
      dirty: false,
      name: "Prod",
      host: "prod.example.com",
      port: "2222",
      username: "root",
      groupId: "g1",
      jumpHost: "j1",
      tags: ["a", "b"],
      sudoPassword: "",
      hasSudoPassword: true,
      sudoPasswordUnknown: true,
      localNotes: "some notes",
      postIngestPolicy: "strict",
      aiInjectionCheckEnabled: true,
      sftpServerPath: "/usr/lib/sftp-server",
      startDirectory: "~/work",
      auth: { kind: "password", value: "", saved: true, remove: false },
    });
  });

  it("load maps auth kinds (no_password, identity_file, private_key, agent)", () => {
    const auth = (patch: Partial<ServerDto>) =>
      serverFormReducer(fresh(), { type: "load", server: { ...server, ...patch } }).auth;
    expect(auth({ authKind: "no_password" })).toEqual({ kind: "password", value: "", saved: false, remove: false });
    expect(auth({ authKind: "identity_file", identityFilePath: "/k" })).toEqual({
      kind: "identityFile",
      path: "/k",
      passphrase: "",
    });
    expect(auth({ authKind: "private_key" })).toEqual(authStateFromKind("privateKey"));
    expect(auth({ authKind: "agent" })).toEqual({ kind: "agent" });
  });

  it("setField writes the value; dirty is only set by markDirty and never cleared by editing back", () => {
    let s = serverFormReducer(fresh(), { type: "load", server });
    s = serverFormReducer(s, { type: "setField", field: "name", value: "Other" });
    expect(s.name).toBe("Other");
    expect(s.dirty).toBe(false);
    s = serverFormReducer(s, { type: "markDirty" });
    expect(s.dirty).toBe(true);
    s = serverFormReducer(s, { type: "setField", field: "name", value: "Prod" });
    expect(s.dirty).toBe(true);
  });

  it("saved clears dirty", () => {
    const s = serverFormReducer(serverFormReducer(fresh(), { type: "markDirty" }), { type: "saved" });
    expect(s.dirty).toBe(false);
  });

  it("setTags replaces the tag list", () => {
    let s = serverFormReducer(fresh(), { type: "setTags", tags: ["x", "y"] });
    expect(s.tags).toEqual(["x", "y"]);
    s = serverFormReducer(s, { type: "setTags", tags: [] });
    expect(s.tags).toEqual([]);
  });

  it("reset returns to create defaults with the given group", () => {
    let s = serverFormReducer(fresh(), { type: "load", server });
    s = serverFormReducer(s, { type: "reset", defaultGroupId: "g9" });
    expect(s).toMatchObject({
      loaded: null,
      name: "",
      host: "",
      port: "22",
      groupId: "g9",
      jumpHost: null,
      tags: [],
      hasSudoPassword: false,
      postIngestPolicy: "balanced",
      aiInjectionCheckEnabled: false,
      auth: { kind: "password", value: "", saved: false, remove: false },
    });
  });
});

describe("toAuthMethodInput", () => {
  it("password: empty means no password on create, unchanged only when saved on update", () => {
    const empty = { kind: "password", value: "", saved: false, remove: false } as const;
    expect(toAuthMethodInput(empty, true)).toEqual({ kind: "noPassword" });
    expect(toAuthMethodInput(empty, false)).toEqual({ kind: "noPassword" });
    expect(toAuthMethodInput({ ...empty, saved: true }, false)).toEqual({ kind: "password", value: null });
    expect(toAuthMethodInput({ ...empty, saved: true }, true)).toEqual({ kind: "noPassword" });
    expect(toAuthMethodInput({ ...empty, saved: true, remove: true }, false)).toEqual({ kind: "noPassword" });
    expect(toAuthMethodInput({ ...empty, value: "pw" }, true)).toEqual({ kind: "password", value: "pw" });
  });

  it("privateKey: empty key is null only on update, empty passphrase always null", () => {
    const a = { kind: "privateKey", keyContent: "", passphrase: "" } as const;
    expect(toAuthMethodInput(a, true)).toEqual({ kind: "privateKey", keyContent: "", passphrase: null });
    expect(toAuthMethodInput(a, false)).toEqual({ kind: "privateKey", keyContent: null, passphrase: null });
    expect(toAuthMethodInput({ ...a, keyContent: "K", passphrase: "p" }, false)).toEqual({
      kind: "privateKey",
      keyContent: "K",
      passphrase: "p",
    });
  });

  it("agent has no payload", () => {
    expect(toAuthMethodInput({ kind: "agent" }, true)).toEqual({ kind: "agent" });
    expect(toAuthMethodInput({ kind: "agent" }, false)).toEqual({ kind: "agent" });
  });

  it("certificate: empty cert and key are null only on update", () => {
    const a = { kind: "certificate", certContent: "", keyContent: "" } as const;
    expect(toAuthMethodInput(a, true)).toEqual({ kind: "certificate", certContent: "", keyContent: "" });
    expect(toAuthMethodInput(a, false)).toEqual({ kind: "certificate", certContent: null, keyContent: null });
  });

  it("identityFile: path always sent untrimmed, empty passphrase null", () => {
    const a = { kind: "identityFile", path: " /k ", passphrase: "" } as const;
    expect(toAuthMethodInput(a, true)).toEqual({ kind: "identityFile", path: " /k ", passphrase: null });
    expect(toAuthMethodInput(a, false)).toEqual({ kind: "identityFile", path: " /k ", passphrase: null });
  });
});
