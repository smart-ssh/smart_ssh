// Issue #1388: long finished command output collapses to an 8-line preview.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it } from "vitest";
import { testI18n } from "../../testI18n";
import { ActionResultView } from "./ActionCard";

const more = (n: number) => testI18n.t("chat.outputMoreLines", { count: n });
const showAll = () => testI18n.t("chat.outputShowAll");
const showLess = () => testI18n.t("chat.outputShowLess");
const lines = (n: number, p = "line") => Array.from({ length: n }, (_, i) => `${p}${i + 1}`).join("\n");

function renderResult(stdout: string, stderr = "") {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ActionResultView
        sessionId="s"
        result={{ kind: "command", stdout, stderr, exitCode: 0, truncated: false, cancelled: false } as never}
      />
    </I18nextProvider>,
  );
}

describe("collapsible command output", () => {
  it("collapses 50 lines to 8 and expands/collapses again", () => {
    const { container } = renderResult(lines(50));
    const pre = () => container.querySelector("pre")!;
    expect(pre().textContent).toBe(lines(8));
    expect(screen.getByText(more(42))).toBeTruthy();
    const btn = screen.getByRole("button", { name: showAll() });
    expect(btn.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(btn);
    expect(pre().textContent).toBe(lines(50));
    const less = screen.getByRole("button", { name: showLess() });
    expect(less.getAttribute("aria-expanded")).toBe("true");
    expect(screen.queryByText(more(42))).toBeNull();
    fireEvent.click(less);
    expect(pre().textContent).toBe(lines(8));
  });

  it("shows no toggle for 8 lines or fewer", () => {
    renderResult(lines(8));
    expect(screen.queryByRole("button", { name: new RegExp(`${showAll()}|${showLess()}`) })).toBeNull();
  });

  it("collapses stderr separately from stdout", () => {
    renderResult(lines(3), lines(20, "err"));
    expect(screen.getAllByRole("button", { name: showAll() })).toHaveLength(1);
    expect(screen.getByText(more(12))).toBeTruthy();
  });

  it("ignores one trailing newline when counting lines", () => {
    const { container } = renderResult(`${lines(50)}\n`);
    expect(screen.getByText(more(42))).toBeTruthy();
    expect(container.querySelector("pre")!.textContent).toBe(lines(8));
  });

  it("shows no toggle for 8 lines plus a trailing newline", () => {
    renderResult(`${lines(8)}\n`);
    expect(screen.queryByRole("button", { name: showAll() })).toBeNull();
  });

  it("uses the singular form for one hidden line", () => {
    renderResult(lines(9));
    expect(screen.getByText("… 1 weitere Zeile")).toBeTruthy();
  });

  it("keeps the truncated marker visible outside the collapsed preview", () => {
    const { container } = renderResult(lines(600, "l"));
    expect(container.querySelector("pre")!.textContent).toBe(lines(8, "l"));
    expect(screen.getByText(`… ${testI18n.t("chat.outputTruncated")}`)).toBeTruthy();
  });
});
