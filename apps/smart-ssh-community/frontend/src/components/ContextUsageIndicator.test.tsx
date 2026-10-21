import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { testI18n } from "../testI18n";
import { describe, expect, it } from "vitest";
import { formatTokensK, isContextNearLimit } from "../contextUsage";
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
    expect(screen.getByTestId("context-usage").className).toContain("text-amber-300");
  });
});
