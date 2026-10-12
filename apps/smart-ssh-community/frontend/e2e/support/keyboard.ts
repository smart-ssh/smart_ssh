// Keyboard-order helpers: walk the Tab order and assert it follows the
// visual (reading) order and that each stop shows a focus indicator.
import { expect, type Page } from "@playwright/test";
import { tabKey } from "./layout";

export interface TabStop {
  label: string;
  box: { left: number; top: number; right: number; bottom: number };
  hasIndicator: boolean;
}

/** Presses Tab `count` times from the current focus and records every stop. */
export async function walkTabOrder(page: Page, count: number): Promise<TabStop[]> {
  const stops: TabStop[] = [];
  for (let i = -1; i < count; i++) {
    // Round -1 only samples the element that already has focus (e.g. a dialog's initial focus).
    if (i >= 0) await page.keyboard.press(tabKey(page));
    const stop = await page.evaluate((initial) => {
      const el = document.activeElement as HTMLElement | null;
      if (!el || el === document.body) return initial ? undefined : null;
      if (initial && el.tabIndex < 0) return undefined;
      const r = el.getBoundingClientRect();
      // Scroll-independent position: add what the scrollable ancestors have scrolled.
      let scrollX = 0;
      let scrollY = 0;
      for (let n = el.parentElement; n; n = n.parentElement) {
        scrollX += n.scrollLeft;
        scrollY += n.scrollTop;
      }
      const cs = getComputedStyle(el);
      const outline = cs.outlineStyle !== "none" && parseFloat(cs.outlineWidth) > 0;
      const shadow = cs.boxShadow !== "none" && cs.boxShadow !== "";
      const label =
        el.getAttribute("aria-label") ?? el.getAttribute("title") ?? (el.innerText || el.getAttribute("name") || el.getAttribute("type") || el.tagName).trim().slice(0, 40);
      return {
        label: `${el.tagName.toLowerCase()} "${label.replace(/\s+/g, " ")}"`,
        box: { left: r.left + scrollX, top: r.top + scrollY, right: r.right + scrollX, bottom: r.bottom + scrollY },
        hasIndicator: outline || shadow,
      };
    }, i < 0);
    if (stop === undefined) continue;
    if (!stop) break; // focus left the document (browser chrome): the cycle is complete
    if (stops.some((s) => s.label === stop.label && s.box.top === stop.box.top && s.box.left === stop.box.left)) break;
    stops.push(stop);
  }
  return stops;
}

/**
 * Each stop is at or below the previous one (8 px tolerance for items on the
 * same row), unless it moved into a column further right (a new pane).
 */
export function assertVisualOrder(stops: TabStop[]): void {
  expect(stops.length, "no Tab stops found").toBeGreaterThan(0);
  for (let i = 1; i < stops.length; i++) {
    const prev = stops[i - 1];
    const cur = stops[i];
    const sameOrBelow = cur.box.top >= prev.box.top - 8;
    const newColumn = cur.box.left >= prev.box.right - 1;
    expect(
      sameOrBelow || newColumn,
      `Tab stop ${i} ${cur.label} (${JSON.stringify(cur.box)}) jumps back above ${prev.label} (${JSON.stringify(prev.box)})`,
    ).toBe(true);
  }
  const missing = stops.filter((s) => !s.hasIndicator).map((s) => s.label);
  expect(missing, "Tab stops without a visible focus indicator").toEqual([]);
}
