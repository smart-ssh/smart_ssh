import { describe, expect, it } from "vitest";
import {
  DiffTooLargeError,
  diffLines,
  isDiffTooLargeToCompute,
  MAX_DIFF_INPUT_BYTES,
  MAX_DIFF_LINE_PRODUCT,
  shortNoteDiff,
} from "./textDiff";

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

  /// spec-reviewer-Fund (Review dieses Schritts): der Byte-Cap allein lässt
  /// sich mit vielen kurzen Zeilen umgehen — weit unter dem Byte-Cap, aber
  /// mit einer DP-Tabellengröße, die den Confirm-Dialog-Renderer trotzdem
  /// einfrieren könnte. Der Zeilenanzahl-Produkt-Cap muss das unabhängig
  /// vom Byte-Cap erkennen.
  it("is true for many short lines that stay well under the byte cap but exceed the line-product cap", () => {
    const manyShortLines = Array.from({ length: 3000 }, (_, i) => `${i}`).join("\n");
    expect(new TextEncoder().encode(manyShortLines).length).toBeLessThan(MAX_DIFF_INPUT_BYTES);
    expect(isDiffTooLargeToCompute(manyShortLines, manyShortLines)).toBe(true);
  });

  it("is false for line counts just under the line-product cap", () => {
    // 2000 x 2000 = 4_000_000, exakt MAX_DIFF_LINE_PRODUCT — knapp darunter
    // (1999 Zeilen je Seite) darf nicht als zu groß gelten.
    const lines = Array.from({ length: 1999 }, (_, i) => `${i}`).join("\n");
    expect(isDiffTooLargeToCompute(lines, lines)).toBe(false);
  });
});

// spec-reviewer-Fund: der Zeilenanzahl-Produkt-Cap muss auch direkt in
// `diffLines`/`shortNoteDiff` greifen, nicht nur über `isDiffTooLargeToCompute`
// beim Aufrufer — eine Verteidigungslinie, die kein künftiger Aufrufer
// versehentlich umgehen kann.
describe("diffLines line-product guard", () => {
  it("throws DiffTooLargeError instead of allocating a huge DP table", () => {
    const manyShortLines = Array.from(
      { length: Math.ceil(Math.sqrt(MAX_DIFF_LINE_PRODUCT)) + 1 },
      (_, i) => `${i}`,
    ).join("\n");
    expect(() => diffLines(manyShortLines, manyShortLines)).toThrow(DiffTooLargeError);
  });

  it("does not throw for line counts under the product cap", () => {
    expect(() => diffLines("a\nb\nc", "a\nb\nd")).not.toThrow();
  });
});
