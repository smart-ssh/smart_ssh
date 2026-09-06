// Spec 0046, Fund 3: über dem Größen-Cap wird kein O(n·m)-Zeilen-Diff mehr
// berechnet — Hinweis + Größenangabe statt Diff, analog zum Binärdatei-Fall.
// Unter dem Cap bleibt das Verhalten (normaler Zeilen-Diff) unverändert.
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it } from "vitest";
import { MAX_DIFF_INPUT_BYTES } from "../textDiff";
import { testI18n } from "../testI18n";
import { NoteDiffPreview } from "./NoteDiffPreview";

function renderPreview(previousContent: string | null, newContent: string) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <NoteDiffPreview previousContent={previousContent} newContent={newContent} />
    </I18nextProvider>,
  );
}

describe("NoteDiffPreview size cap (Spec 0046, Fund 3)", () => {
  it("renders a normal line diff for content under the cap", () => {
    renderPreview("a\nb\nc", "a\nb\nd");

    expect(screen.getByText("d")).toBeInTheDocument();
    expect(screen.queryByText(/zu groß|too large/i)).toBeNull();
  });

  it("skips the line diff and shows a size hint when the new content exceeds the cap", () => {
    const oversized = "x".repeat(MAX_DIFF_INPUT_BYTES + 1);
    renderPreview("short before", oversized);

    expect(screen.getByText(/zu groß für Zeilen-Diff|too large for a line diff/i)).toBeInTheDocument();
  });

  it("skips the line diff and shows a size hint when the previous content exceeds the cap", () => {
    const oversized = "x".repeat(MAX_DIFF_INPUT_BYTES + 1);
    renderPreview(oversized, "short after");

    expect(screen.getByText(/zu groß für Zeilen-Diff|too large for a line diff/i)).toBeInTheDocument();
  });

  it("skips the line diff for many short lines that stay under the byte cap but exceed the line-product cap", () => {
    // spec-reviewer-Fund: der Byte-Cap allein hätte diesen Fall
    // durchgelassen (weit unter 256 KB) und die volle O(n·m)-DP-Tabelle
    // berechnet.
    const manyShortLines = Array.from({ length: 3000 }, (_, i) => `${i}`).join("\n");
    renderPreview(manyShortLines, manyShortLines);

    expect(screen.getByText(/zu groß für Zeilen-Diff|too large for a line diff/i)).toBeInTheDocument();
  });

  it("still allows the write itself: the hint does not disable anything outside this preview", () => {
    // Die Komponente selbst enthält keinen Schreib-Button — dieser Test
    // dokumentiert die Invariante explizit: die Größen-Hinweis-Ansicht
    // rendert nur einen Absatz, keine deaktivierten Controls.
    const oversized = "x".repeat(MAX_DIFF_INPUT_BYTES + 1);
    const { container } = renderPreview("short", oversized);

    expect(container.querySelectorAll("button, input")).toHaveLength(0);
  });
});
