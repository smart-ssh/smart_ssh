import { describe, expect, it } from "vitest";
import type { DeletePreviewDto, RemoteEntryDto } from "../../api/types";
import {
  fileOperationsReducer,
  initialFileOperationsState,
  type FileOperationsState,
} from "./useFileOperations";

const entry = (name: string, isDir = false) => ({ name, path: `/srv/${name}`, isDir }) as RemoteEntryDto;
const preview = { fileCount: 3, dirCount: 2 } as DeletePreviewDto;
const withDialog = (dialog: FileOperationsState["dialog"]): FileOperationsState => ({
  ...initialFileOperationsState,
  dialog,
});

describe("fileOperationsReducer", () => {
  it("starts with no dialog and nothing cut", () => {
    expect(initialFileOperationsState).toEqual({ dialog: { kind: "none" }, cutEntry: null });
  });

  it("opens each dialog kind, replacing the one that is open", () => {
    const a = entry("a");
    let state = fileOperationsReducer(initialFileOperationsState, {
      type: "open",
      dialog: { kind: "rename", entry: a, value: "a" },
    });
    expect(state.dialog).toEqual({ kind: "rename", entry: a, value: "a" });
    state = fileOperationsReducer(state, { type: "open", dialog: { kind: "chmod", entry: a } });
    expect(state.dialog).toEqual({ kind: "chmod", entry: a });
    state = fileOperationsReducer(state, { type: "open", dialog: { kind: "mkdir", value: "", error: null } });
    expect(state.dialog.kind).toBe("mkdir");
  });

  it("closes the dialog and keeps the cut entry", () => {
    const state = {
      dialog: { kind: "properties", entry: entry("a") } as const,
      cutEntry: entry("c"),
    };
    expect(fileOperationsReducer(state, { type: "close" })).toEqual({
      dialog: { kind: "none" },
      cutEntry: entry("c"),
    });
    expect(fileOperationsReducer(initialFileOperationsState, { type: "close" })).toBe(initialFileOperationsState);
  });

  it("updates the rename value only while renaming", () => {
    const a = entry("a");
    const renaming = withDialog({ kind: "rename", entry: a, value: "a" });
    expect(fileOperationsReducer(renaming, { type: "renameChanged", value: "b" }).dialog).toEqual({
      kind: "rename",
      entry: a,
      value: "b",
    });
    const other = withDialog({ kind: "chmod", entry: a });
    expect(fileOperationsReducer(other, { type: "renameChanged", value: "b" })).toBe(other);
  });

  it("typing a name clears the error; a rejected submit sets it (mkdir and new file)", () => {
    for (const kind of ["mkdir", "newFile"] as const) {
      const state = withDialog({ kind, value: "a/b", error: null });
      const invalid = fileOperationsReducer(state, { type: "nameInvalid", error: "bad" });
      expect(invalid.dialog).toEqual({ kind, value: "a/b", error: "bad" });
      const typed = fileOperationsReducer(invalid, { type: "nameChanged", value: "ab" });
      expect(typed.dialog).toEqual({ kind, value: "ab", error: null });
    }
    const other = withDialog({ kind: "chown", entry: entry("a") });
    expect(fileOperationsReducer(other, { type: "nameInvalid", error: "x" })).toBe(other);
    expect(fileOperationsReducer(other, { type: "nameChanged", value: "x" })).toBe(other);
  });

  it("applies a delete preview only to the matching delete target", () => {
    const dir = entry("dir", true);
    const state = withDialog({ kind: "delete", entry: dir, preview: null });
    expect(fileOperationsReducer(state, { type: "deletePreview", path: dir.path, preview }).dialog).toEqual({
      kind: "delete",
      entry: dir,
      preview,
    });
    expect(fileOperationsReducer(state, { type: "deletePreview", path: "/srv/other", preview })).toBe(state);
    const closed = initialFileOperationsState;
    expect(fileOperationsReducer(closed, { type: "deletePreview", path: dir.path, preview })).toBe(closed);
  });

  it("opens a move collision as the only dialog", () => {
    const state = fileOperationsReducer(withDialog({ kind: "rename", entry: entry("a"), value: "b" }), {
      type: "open",
      dialog: { kind: "moveCollision", from: "/srv/a", to: "/srv/b", moveKind: "rename", targetIsDir: false },
    });
    expect(state.dialog).toMatchObject({ kind: "moveCollision", to: "/srv/b" });
  });

  it("cuts and clears the cut entry independently of the dialog", () => {
    const c = entry("c");
    const cut = fileOperationsReducer(withDialog({ kind: "properties", entry: c }), { type: "cut", entry: c });
    expect(cut.cutEntry).toBe(c);
    expect(cut.dialog.kind).toBe("properties");
    expect(fileOperationsReducer(cut, { type: "clearCut" }).cutEntry).toBeNull();
    expect(fileOperationsReducer(initialFileOperationsState, { type: "clearCut" })).toBe(
      initialFileOperationsState,
    );
  });
});
