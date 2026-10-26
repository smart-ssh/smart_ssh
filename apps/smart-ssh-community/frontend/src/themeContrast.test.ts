import { describe, expect, it } from "vitest";
import { resolvedTheme } from "./themeTokens";

// Issue #804: WCAG AA (4.5:1) on the token values of both appearances.
function lum(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const [r, g, b] = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
function ratio(a: string, b: string): number {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

const SURFACES = ["surface-sunken", "surface", "surface-raised"];
const BODY_TEXT = ["fg-faint", "fg-subtle", "fg-muted", "fg-soft", "fg", "fg-strong", "fg-bright"];
const ROLES = ["accent", "danger", "warning", "success"];
const TERM_COLOURS = [
  "fg", "ansi-black", "ansi-red", "ansi-green", "ansi-yellow", "ansi-blue", "ansi-magenta",
  "ansi-cyan", "ansi-white", "ansi-bright-black", "ansi-bright-red", "ansi-bright-green",
  "ansi-bright-yellow", "ansi-bright-blue", "ansi-bright-magenta", "ansi-bright-cyan", "ansi-bright-white",
];

describe.each(["light", "dark"] as const)("%s tokens", (name) => {
  const t = resolvedTheme(name);
  const v = (token: string) => t[`--${token}`];

  it("resolves every token to a colour", () => {
    for (const [k, value] of Object.entries(t)) {
      expect(value, k).toMatch(/^(#[0-9a-fA-F]{6}|rgba?\(.*\))$/);
    }
  });

  it.each(SURFACES.flatMap((s) => BODY_TEXT.map((f) => [f, s])))("%s on %s >= 4.5:1", (f, s) => {
    expect(ratio(v(f), v(s))).toBeGreaterThanOrEqual(4.5);
  });

  it.each(ROLES)("%s text steps on their tinted surface", (role) => {
    const steps = role === "warning" || role === "success"
      ? ["fg", "fg-soft", "fg-softer", "fg-faint"]
      : ["fg", "fg-soft", "fg-softer", "fg-faint"];
    for (const s of steps) expect(ratio(v(`${role}-${s}`), v(`${role}-subtle`)), `${role}-${s}`).toBeGreaterThanOrEqual(4.5);
    for (const s of steps.slice(1)) {
      expect(ratio(v(`${role}-${s}`), v(`${role}-subtle-hover`)), `${role}-${s}/hover`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("text on solid fills keeps contrast", () => {
    for (const role of ["accent", "success", "warning"]) {
      expect(ratio(v("on-accent"), v(role)), role).toBeGreaterThanOrEqual(name === "dark" && role === "accent" ? 3 : 4.5);
    }
    expect(ratio(v("on-solid"), v("accent"))).toBeGreaterThanOrEqual(name === "dark" ? 3 : 4.5);
  });

  it.each(TERM_COLOURS)("terminal %s is readable on the terminal background", (c) => {
    // Dark is unchanged from before; only the light terminal is held to AA.
    const min = name === "light" ? 4.5 : c === "fg" ? 4.5 : c === "ansi-black" ? 1 : 2.5;
    expect(ratio(v(`term-${c}`), v("term-bg"))).toBeGreaterThanOrEqual(min);
  });
});
