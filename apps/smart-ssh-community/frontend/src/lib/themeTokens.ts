// Test helper (issue #804): reads the theme files and resolves the token
// values the way the browser does (light overrides dark, var() chains).
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const dir = resolve(process.cwd(), "src/theme");

/** Variable declarations of the first rule block of a theme file. */
function declarations(file: string): Record<string, string> {
  const css = readFileSync(resolve(dir, file), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  const body = css.slice(css.indexOf("{") + 1, css.lastIndexOf("}"));
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/(--[a-z0-9-]+):\s*([^;]+);/g)) out[m[1]] = m[2].trim();
  return out;
}

export const THEME_FILES = { dark: "dark.css", light: "light.css" } as const;
export const darkDeclarations = declarations(THEME_FILES.dark);
export const lightDeclarations = declarations(THEME_FILES.light);

/** Fully resolved variables of a theme (`light` falls back to `dark` values). */
export function resolvedTheme(name: keyof typeof THEME_FILES): Record<string, string> {
  const merged = name === "dark" ? darkDeclarations : { ...darkDeclarations, ...lightDeclarations };
  const resolveValue = (v: string): string =>
    v.replace(/var\((--[a-z0-9-]+)\)/g, (_, n: string) => resolveValue(merged[n] ?? ""));
  return Object.fromEntries(Object.entries(merged).map(([k, v]) => [k, resolveValue(v)]));
}
