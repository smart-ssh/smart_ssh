// Spec-Reviewer-Fund (Spec 0053, Review dieses Schritts): die
// Validierungslogik in `layoutSettings.ts` ist reine, trivial testbare
// Logik (die Projekt-Konvention sieht dafür eigene `*.test.ts`-Dateien
// vor), war aber ungetestet — insbesondere die im Review geprüften
// Sonderfälle (Array statt Objekt, String "Infinity", extrem großer Wert).
import { describe, expect, it, vi } from "vitest";

const storeGet = vi.fn();
const storeSet = vi.fn();
const storeSave = vi.fn();

vi.mock("./i18n", () => ({
  settingsStore: () =>
    Promise.resolve({ get: storeGet, set: storeSet, save: storeSave }),
}));

// Import erst NACH dem Mock, damit `settingsStore` bereits ersetzt ist.
const { loadFileManagerColumnWidths, loadAiSshSplitWidthPx } = await import("./layoutSettings");

describe("loadFileManagerColumnWidths", () => {
  it("returns an empty object when nothing is stored", async () => {
    storeGet.mockResolvedValue(undefined);
    expect(await loadFileManagerColumnWidths()).toEqual({});
  });

  it("keeps only the fields that are valid, positive, finite numbers", async () => {
    storeGet.mockResolvedValue({ size: 200, permissions: -5, modified: NaN });
    expect(await loadFileManagerColumnWidths()).toEqual({ size: 200 });
  });

  it("ignores a string 'Infinity' instead of treating it as the number", async () => {
    storeGet.mockResolvedValue({ size: "Infinity" });
    expect(await loadFileManagerColumnWidths()).toEqual({});
  });

  it("ignores an implausibly large value (corrupted settings.json)", async () => {
    storeGet.mockResolvedValue({ size: 1e20 });
    expect(await loadFileManagerColumnWidths()).toEqual({});
  });

  it("treats a non-object (e.g. an array) as if nothing were stored", async () => {
    storeGet.mockResolvedValue([90, 90, 150]);
    expect(await loadFileManagerColumnWidths()).toEqual({});
  });
});

describe("loadAiSshSplitWidthPx", () => {
  it("returns null when nothing is stored", async () => {
    storeGet.mockResolvedValue(undefined);
    expect(await loadAiSshSplitWidthPx()).toBeNull();
  });

  it("returns the stored value when it is a valid width", async () => {
    storeGet.mockResolvedValue(500);
    expect(await loadAiSshSplitWidthPx()).toBe(500);
  });

  it("returns null for a non-numeric stored value", async () => {
    storeGet.mockResolvedValue("500px");
    expect(await loadAiSshSplitWidthPx()).toBeNull();
  });

  it("returns null for an implausibly large stored value", async () => {
    storeGet.mockResolvedValue(Number.MAX_VALUE);
    expect(await loadAiSshSplitWidthPx()).toBeNull();
  });
});
