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
  // Page ground of the active theme, so browser-hosted system bars (Android
  // WebView) follow the appearance; empty where no stylesheet is loaded.
  meta.content = getComputedStyle(root).getPropertyValue("--surface-sunken").trim();
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
