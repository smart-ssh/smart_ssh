import { beforeEach, describe, expect, it, vi } from "vitest";

const storeData = new Map<string, unknown>();
const save = vi.fn(async () => {});
vi.mock("./i18n", () => ({
  settingsStore: async () => ({
    get: async (k: string) => storeData.get(k),
    set: async (k: string, v: unknown) => void storeData.set(k, v),
    save,
  }),
}));
const setNativeTheme = vi.fn(async (_t: unknown) => {});
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setTheme: setNativeTheme }),
}));

import { darkDeclarations, lightDeclarations } from "./themeTokens";
import {
  THEME_CACHE_KEY,
  applyStoredTheme,
  applyThemeChoice,
  getResolvedTheme,
  parseThemeChoice,
  resolveTheme,
  setThemeChoice,
} from "./theme";

describe("resolveTheme", () => {
  it("follows the system only for 'system'", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });
});

describe("parseThemeChoice", () => {
  it("defaults to system for anything unknown", () => {
    expect(parseThemeChoice(undefined)).toBe("system");
    expect(parseThemeChoice("blue")).toBe("system");
    expect(parseThemeChoice("light")).toBe("light");
    expect(parseThemeChoice("dark")).toBe("dark");
  });
});

describe("persistence and application", () => {
  beforeEach(() => {
    storeData.clear();
    save.mockClear();
    setNativeTheme.mockClear();
    window.localStorage.clear();
    vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("dark") }));
  });

  it("applies immediately and persists to store and cache", async () => {
    await setThemeChoice("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.style.colorScheme).toBe("light");
    expect(storeData.get("theme")).toBe("light");
    expect(save).toHaveBeenCalled();
    expect(window.localStorage.getItem(THEME_CACHE_KEY)).toBe("light");
    expect(setNativeTheme).toHaveBeenCalledWith("light");
  });

  it("system hands the decision to the OS and resolves via the media query", () => {
    applyThemeChoice("system");
    expect(getResolvedTheme()).toBe("dark");
    expect(setNativeTheme).toHaveBeenCalledWith(null);
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    applyThemeChoice("system");
    expect(getResolvedTheme()).toBe("light");
  });

  it("restores the stored choice, defaulting to system when nothing is stored", async () => {
    await applyStoredTheme();
    expect(window.localStorage.getItem(THEME_CACHE_KEY)).toBe("system");
    storeData.set("theme", "light");
    await applyStoredTheme();
    expect(getResolvedTheme()).toBe("light");
  });
});

describe("theme files", () => {
  it("light defines every variable that has a literal value in dark (palette, literals, terminal)", () => {
    // Semantic tokens that only reference a palette primitive follow the palette.
    const literal = Object.entries(darkDeclarations)
      .filter(([, v]) => !v.startsWith("var("))
      .map(([k]) => k);
    expect(literal.filter((k) => !(k in lightDeclarations))).toEqual([]);
  });

  it("light does not define variables unknown to dark", () => {
    expect(Object.keys(lightDeclarations).filter((k) => !(k in darkDeclarations))).toEqual([]);
  });
});
