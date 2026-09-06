import { describe, expect, it } from "vitest";
import { diffLines, isDiffTooLargeToCompute, MAX_DIFF_INPUT_BYTES, shortNoteDiff } from "./textDiff";

// Spec 0019, Abschnitt 4 — reine Diff-Logik, losgelöst von der Darstellung.

describe("diffLines", () => {
  it("markiert identische Texte vollständig als unverändert", () => {
    const result = diffLines("a\nb\nc", "a\nb\nc");
    expect(result).toEqual([
      { type: "unchanged", text: "a" },
      { type: "unchanged", text: "b" },
      { type: "unchanged", text: "c" },
    ]);
  });

  it("erkennt eine reine Ergänzung als added-Zeile", () => {
    const result = diffLines("a\nb", "a\nb\nc");
    expect(result).toEqual([
      { type: "unchanged", text: "a" },
      { type: "unchanged", text: "b" },
      { type: "added", text: "c" },
    ]);
  });

  it("erkennt eine reine Entfernung als removed-Zeile", () => {
    const result = diffLines("a\nb\nc", "a\nc");
    expect(result).toEqual([
      { type: "unchanged", text: "a" },
      { type: "removed", text: "b" },
      { type: "unchanged", text: "c" },
    ]);
  });

  it("behandelt einen leeren Ausgangstext als reine Ergänzung", () => {
    const result = diffLines("", "neu");
    expect(result).toEqual([{ type: "added", text: "neu" }]);
  });
});

describe("shortNoteDiff", () => {
  it("lässt unveränderte Zeilen weg (kurze Vorschau)", () => {
    const result = shortNoteDiff("a\nb\nc", "a\nb\nc\nd");
    expect(result).toEqual([{ type: "added", text: "d" }]);
  });

  it("behandelt null (keine Zielauflösung) wie einen leeren Ausgangstext", () => {
    const result = shortNoteDiff(null, "erste Notiz");
    expect(result).toEqual([{ type: "added", text: "erste Notiz" }]);
  });
});

// Spec 0046, Fund 3: Größen-Cap für die Diff-Berechnung.
describe("isDiffTooLargeToCompute", () => {
  it("is false for short content well under the cap", () => {
    expect(isDiffTooLargeToCompute("a\nb\nc", "a\nb\nd")).toBe(false);
  });

  it("is true when the previous content alone exceeds the cap", () => {
    const oversizedBefore = "x".repeat(MAX_DIFF_INPUT_BYTES + 1);
    expect(isDiffTooLargeToCompute(oversizedBefore, "short")).toBe(true);
  });

  it("is true when the new content alone exceeds the cap", () => {
    const oversizedAfter = "x".repeat(MAX_DIFF_INPUT_BYTES + 1);
    expect(isDiffTooLargeToCompute("short", oversizedAfter)).toBe(true);
  });

  it("is false right at the cap boundary", () => {
    const atCap = "x".repeat(MAX_DIFF_INPUT_BYTES);
    expect(isDiffTooLargeToCompute(atCap, atCap)).toBe(false);
  });
});
