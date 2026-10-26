import { getCurrentWindow } from "@tauri-apps/api/window";
import { settingsStore } from "./i18n";

/** Issue #804: appearance setting (System / Light / Dark). The choice is
 * kept in the settings store next to the language. Because that store is not
 * readable before the unlock in password mode (Spec 0101), the last choice
 * is mirrored into `localStorage` so the very first paint — including the
 * startup and unlock screens — already uses the right theme. */
export const THEME_CHOICES = ["system", "light", "dark"] as const;
export type ThemeChoice = (typeof THEME_CHOICES)[number];
export type ResolvedTheme = "light" | "dark";
export const DEFAULT_THEME_CHOICE: ThemeChoice = "system";

const STORE_KEY = "theme";
/** Same key as in `public/theme-init.js` (pre-paint script). */
export const THEME_CACHE_KEY = "smart-ssh.theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";
/** Page background per theme, mirrored into `<meta name="theme-color">` so
 * browser-hosted system bars (Android WebView) follow the appearance. Keep in
 * sync with `--color-slate-950` in `index.css`. */
const THEME_COLOR: Record<ResolvedTheme, string> = { dark: "#090c14", light: "#f4f6fa" };

/** Anything but the three known values (missing, corrupt, hand-edited) is the default. */
export function parseThemeChoice(value: unknown): ThemeChoice {
  return (THEME_CHOICES as readonly unknown[]).includes(value)
    ? (value as ThemeChoice)
    : DEFAULT_THEME_CHOICE;
}

export function resolveTheme(choice: ThemeChoice, systemPrefersDark: boolean): ResolvedTheme {
  if (choice === "light") return "light";
  if (choice === "dark") return "dark";
  return systemPrefersDark ? "dark" : "light";
}

function systemPrefersDark(): boolean {
  // No matchMedia (very old engines): the app's historical look is dark.
  return typeof window === "undefined" || typeof window.matchMedia !== "function"
    ? true
    : window.matchMedia(DARK_QUERY).matches;
}

let currentChoice: ThemeChoice = DEFAULT_THEME_CHOICE;
const listeners = new Set<() => void>();

export function getThemeChoice(): ThemeChoice {
  return currentChoice;
}

/** The theme currently applied to the document. */
export function getResolvedTheme(): ResolvedTheme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

export function subscribeTheme(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function readCachedChoice(): ThemeChoice {
  try {
    return parseThemeChoice(window.localStorage.getItem(THEME_CACHE_KEY));
  } catch {
    return DEFAULT_THEME_CHOICE;
  }
}

function writeCachedChoice(choice: ThemeChoice): void {
  try {
    window.localStorage.setItem(THEME_CACHE_KEY, choice);
  } catch (err) {
    // The cache only avoids a flash at the next start; the store stays authoritative.
    console.error("Theme cache could not be written:", err);
  }
}

function syncNativeTheme(choice: ThemeChoice): void {
  // `null` hands the decision back to the OS, so decorations follow it live.
  const target = choice === "system" ? null : choice;
  try {
    void getCurrentWindow()
      .setTheme(target)
      .catch((err: unknown) => console.error("Window theme could not be set:", err));
  } catch (err) {
    // No Tauri window (browser harness, unit tests).
    console.debug("Window theme not available:", err);
  }
}

/** Applies a choice to the document (and the native window) immediately. */
export function applyThemeChoice(choice: ThemeChoice): void {
  currentChoice = choice;
  const resolved = resolveTheme(choice, systemPrefersDark());
  const root = document.documentElement;
  root.dataset.theme = resolved;
  root.style.colorScheme = resolved;
  let meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
  if (!meta) {
    meta = document.createElement("meta");
    meta.name = "theme-color";
    document.head.appendChild(meta);
  }
  meta.content = THEME_COLOR[resolved];
  syncNativeTheme(choice);
  listeners.forEach((l) => l());
}

let mediaListenerInstalled = false;

/** Before the first render: cached choice + live OS-switch tracking. */
export function initTheme(): void {
  applyThemeChoice(readCachedChoice());
  if (mediaListenerInstalled || typeof window.matchMedia !== "function") return;
  mediaListenerInstalled = true;
  window.matchMedia(DARK_QUERY).addEventListener("change", () => {
    if (currentChoice === "system") applyThemeChoice("system");
  });
}

export async function loadThemeChoice(): Promise<ThemeChoice> {
  const store = await settingsStore();
  return parseThemeChoice(await store.get<unknown>(STORE_KEY));
}

/** Once the store is readable (startup, or after the unlock): the stored choice wins. */
export async function applyStoredTheme(): Promise<void> {
  const choice = await loadThemeChoice();
  writeCachedChoice(choice);
  applyThemeChoice(choice);
}

export async function setThemeChoice(choice: ThemeChoice): Promise<void> {
  applyThemeChoice(choice);
  writeCachedChoice(choice);
  const store = await settingsStore();
  await store.set(STORE_KEY, choice);
  await store.save();
}

/** xterm.js reads no CSS variables, so the terminal gets explicit palettes. */
export interface TerminalPalette {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export const TERMINAL_THEMES: Record<ResolvedTheme, TerminalPalette> = {
  dark: {
    background: "#0c1120",
    foreground: "#dfe3ee",
    cursor: "#6c7cf5",
    cursorAccent: "#0c1120",
    selectionBackground: "rgba(108,124,245,0.35)",
    black: "#0c1120",
    red: "#e5484d",
    green: "#3fb950",
    yellow: "#e0a12c",
    blue: "#6c7cf5",
    magenta: "#a5b0ff",
    cyan: "#5980a6",
    white: "#8b93a8",
    brightBlack: "#4f596e",
    brightRed: "#f0918f",
    brightGreen: "#8fd79a",
    brightYellow: "#f0c46b",
    brightBlue: "#a5b0ff",
    brightMagenta: "#c9d0ff",
    brightCyan: "#9fc0dd",
    brightWhite: "#e6e9f2",
  },
  // Every normal colour reaches >= 4.5:1 on the background (themeContrast.test.ts).
  light: {
    background: "#fafbfd",
    foreground: "#1c2233",
    cursor: "#4350c8",
    cursorAccent: "#fafbfd",
    selectionBackground: "rgba(67,80,200,0.25)",
    black: "#1c2233",
    red: "#b3262b",
    green: "#1a7f37",
    yellow: "#8a5a00",
    blue: "#3b45a8",
    magenta: "#8a2fa0",
    cyan: "#1f6a80",
    white: "#4f596e",
    brightBlack: "#566079",
    brightRed: "#c0282d",
    brightGreen: "#1f6f35",
    brightYellow: "#7a5000",
    brightBlue: "#2f3aa0",
    brightMagenta: "#7a2a8f",
    brightCyan: "#1a6075",
    brightWhite: "#2c3346",
  },
};
