import { describe, expect, it, vi } from "vitest";

const storeGet = vi.fn();
const storeSet = vi.fn();
const storeSave = vi.fn();

vi.mock("./i18n", () => ({
  settingsStore: () => Promise.resolve({ get: storeGet, set: storeSet, save: storeSave }),
}));

// Import erst NACH dem Mock, damit `settingsStore` bereits ersetzt ist.
const { appForFileName, loadFileTypeApps, normalizeExtension, saveFileTypeApps } = await import(
  "./fileTypeSettings"
);

describe("normalizeExtension", () => {
  it("lowercases and adds a leading dot if missing", () => {
    expect(normalizeExtension("CONF")).toBe(".conf");
    expect(normalizeExtension(".Log")).toBe(".log");
    expect(normalizeExtension("  .txt  ")).toBe(".txt");
  });

  it("returns an empty string for an empty/whitespace-only input", () => {
    expect(normalizeExtension("")).toBe("");
    expect(normalizeExtension("   ")).toBe("");
  });
});

describe("loadFileTypeApps", () => {
  it("returns an empty object when nothing is stored", async () => {
    storeGet.mockResolvedValue(undefined);
    expect(await loadFileTypeApps()).toEqual({});
  });

  it("normalizes stored keys and keeps only non-empty string values", async () => {
    storeGet.mockResolvedValue({ CONF: "/usr/bin/code", ".log": "", "  ": "/bin/x" });
    expect(await loadFileTypeApps()).toEqual({ ".conf": "/usr/bin/code" });
  });

  it("treats a non-object (e.g. an array) as if nothing were stored", async () => {
    storeGet.mockResolvedValue([".conf", "/usr/bin/code"]);
    expect(await loadFileTypeApps()).toEqual({});
  });

  it("ignores a non-string value (corrupted settings.json)", async () => {
    storeGet.mockResolvedValue({ ".conf": 42 });
    expect(await loadFileTypeApps()).toEqual({});
  });
});

describe("saveFileTypeApps", () => {
  it("writes and saves the given map", async () => {
    await saveFileTypeApps({ ".conf": "/usr/bin/code" });
    expect(storeSet).toHaveBeenCalledWith("fileTypeDefaultApps", { ".conf": "/usr/bin/code" });
    expect(storeSave).toHaveBeenCalled();
  });
});

describe("appForFileName", () => {
  const apps = { ".conf": "/usr/bin/code", ".log": "/usr/bin/less" };

  it("finds the app for a matching extension", () => {
    expect(appForFileName(apps, "nginx.conf")).toBe("/usr/bin/code");
  });

  it("is case-insensitive on the extension", () => {
    expect(appForFileName(apps, "nginx.CONF")).toBe("/usr/bin/code");
  });

  it("returns null when there is no mapping for the extension", () => {
    expect(appForFileName(apps, "readme.md")).toBeNull();
  });

  it("returns null for a file without an extension", () => {
    expect(appForFileName(apps, "Makefile")).toBeNull();
  });

  it("returns null for a dotfile with no extension of its own (e.g. .bashrc)", () => {
    expect(appForFileName(apps, ".bashrc")).toBeNull();
  });
});
