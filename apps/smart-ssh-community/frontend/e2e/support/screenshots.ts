// Before/after images of a named screen at each size in `SCREENSHOT_SIZES`, for review PRs.
// Writes `e2e/screenshots/<label>/<name>-<width>x<height>.png` (git-ignored).
// Run with `SCREENSHOT_LABEL=before` / `after` to keep two sets side by side;
// the label defaults to `current`.
import fs from "node:fs";
import path from "node:path";
import type { Page } from "@playwright/test";

export const SCREENSHOT_SIZES = [
  { width: 1280, height: 800 },
  { width: 800, height: 560 },
  { width: 390, height: 844 },
] as const;

/**
 * Captures the page as it is at each of `SCREENSHOT_SIZES` and restores the
 * original viewport. Returns the written file paths. `name` is the screen's
 * name (letters, digits, `-`, `_`).
 */
export async function captureScreen(page: Page, name: string): Promise<string[]> {
  if (!/^[\w-]+$/.test(name)) throw new Error(`invalid screenshot name: ${name}`);
  const label = process.env.SCREENSHOT_LABEL ?? "current";
  if (!/^[\w-]+$/.test(label)) throw new Error(`invalid SCREENSHOT_LABEL: ${label}`);
  const dir = path.join(import.meta.dirname, "..", "screenshots", label);
  fs.mkdirSync(dir, { recursive: true });
  const original = page.viewportSize();
  const files: string[] = [];
  try {
    for (const size of SCREENSHOT_SIZES) {
      await page.setViewportSize(size);
      // Let the layout settle after the resize.
      await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
      const file = path.join(dir, `${name}-${size.width}x${size.height}.png`);
      await page.screenshot({ path: file });
      files.push(file);
    }
  } finally {
    if (original) await page.setViewportSize(original);
  }
  return files;
}
