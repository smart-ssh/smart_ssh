import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { testI18n } from "../testI18n";
import { describe, expect, it } from "vitest";
import { contextSegments, formatTokensK, isContextNearLimit } from "../lib/contextUsage";
import { ContextUsageIndicator } from "./ContextUsageIndicator";

const wrap = (n: number) => (
  <I18nextProvider i18n={testI18n}>
    <ContextUsageIndicator inputTokens={n} windowTokens={200_000} />
  </I18nextProvider>
);

describe("context usage", () => {
  it("formats in k", () => {
    expect(formatTokensK(120_000)).toBe("120k");
    expect(formatTokensK(1_500)).toBe("1.5k");
    expect(formatTokensK(900)).toBe("900");
  });

  it("is near the limit from 75%", () => {
    expect(isContextNearLimit(149_999, 200_000)).toBe(false);
    expect(isContextNearLimit(150_000, 200_000)).toBe(true);
    expect(isContextNearLimit(1, 0)).toBe(false);
  });

  it("renders sizes and colours from 75%", () => {
    const { rerender } = render(wrap(120_000));
    expect(screen.getByTestId("context-usage").textContent).toContain("120k");
    expect(screen.getByTestId("context-usage").textContent).toContain("200k");
    expect(screen.getByTestId("context-usage").dataset.nearLimit).toBe("false");
    rerender(wrap(160_000));
    expect(screen.getByTestId("context-usage").dataset.nearLimit).toBe("true");
    expect(screen.getByTestId("context-usage").className).toContain("text-warning-fg-soft");
  });

  it("fills ceil(used / window × 8) segments", () => {
    expect(contextSegments(12_400, 200_000)).toBe(1);
    expect(contextSegments(160_000, 200_000)).toBe(7);
    expect(contextSegments(200_000, 200_000)).toBe(8);
    expect(contextSegments(300_000, 200_000)).toBe(8);
    expect(contextSegments(0, 200_000)).toBe(0);
    expect(contextSegments(5, 0)).toBe(0);
  });

  it("renders 8 aria-hidden segments, 1 filled at 12.4k and 7 filled in warning colour at 160k", () => {
    const { rerender } = render(wrap(12_400));
    const seg = () => screen.getByTestId("context-segments");
    expect(seg().getAttribute("aria-hidden")).toBe("true");
    expect(seg().children).toHaveLength(8);
    expect(seg().querySelectorAll('[data-filled="true"]')).toHaveLength(1);
    expect(screen.getByTestId("context-usage").textContent).toContain("12.4k");
    rerender(wrap(160_000));
    expect(seg().querySelectorAll('[data-filled="true"]')).toHaveLength(7);
    expect(seg().querySelector('[data-filled="true"]')!.className).toContain("bg-warning-fg-soft");
    expect(screen.getByTestId("context-usage").dataset.nearLimit).toBe("true");
  });
});
