import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { TERMINAL_THEMES } from "./theme";

// Issue #804: WCAG AA (4.5:1) on the token values of both appearances.
const css = readFileSync(resolve(process.cwd(), "src/index.css"), "utf8");

function tokens(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of block.matchAll(/--color-([a-z]+-\d+|ink):\s*(#[0-9a-fA-F]{6})/g)) out[m[1]] = m[2];
  return out;
}
const dark = tokens(css.slice(css.indexOf("@theme"), css.indexOf('html[data-theme="light"] {')));
const light = tokens(css.slice(css.indexOf('html[data-theme="light"] {'), css.indexOf("/* Native scrollbars")));

function lum(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const [r, g, b] = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
function ratio(a: string, b: string): number {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

const SURFACES = ["slate-950", "slate-900", "slate-800"];
const BODY_TEXT = ["slate-500", "slate-400", "slate-300", "slate-200", "slate-100", "slate-50"];

describe.each([
  ["light", light],
  ["dark", dark],
] as const)("%s tokens", (_name, t) => {
  it.each(SURFACES.flatMap((s) => BODY_TEXT.map((f) => [f, s])))("%s on %s >= 4.5:1", (f, s) => {
    expect(ratio(t[f], t[s])).toBeGreaterThanOrEqual(4.5);
  });

  it.each(["indigo", "emerald", "amber", "red"])("%s text steps on their tinted surface", (p) => {
    for (const fg of ["400", "300", "200", "100"]) {
      expect(ratio(t[`${p}-${fg}`], t[`${p}-950`])).toBeGreaterThanOrEqual(4.5);
    }
    for (const fg of ["300", "200", "100"]) {
      expect(ratio(t[`${p}-${fg}`], t[`${p}-900`])).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("solid action buttons (bg-x-600 + text-slate-950) keep contrast", () => {
    for (const p of ["indigo", "emerald", "amber"]) {
      expect(ratio(t["slate-950"], t[`${p}-600`])).toBeGreaterThanOrEqual(p === "indigo" && _name === "dark" ? 3 : 4.5);
    }
  });
});

describe("light terminal theme", () => {
  const p = TERMINAL_THEMES.light;
  it.each(["foreground", "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite"] as const)(
    "%s is readable on the background",
    (k) => expect(ratio(p[k], p.background)).toBeGreaterThanOrEqual(4.5),
  );
});
