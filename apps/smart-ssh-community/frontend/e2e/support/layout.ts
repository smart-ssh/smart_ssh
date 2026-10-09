// Layout assertions that check geometry in the real browser instead of
// comparing screenshots, so they give the same answer on every OS. All of
// them retry until they hold or time out (Playwright auto-waiting), none of
// them sleeps.
import { expect, type Locator, type Page } from "@playwright/test";

/** The element's whole box lies inside the viewport (nothing cut off). */
export async function expectFullyInViewport(locator: Locator): Promise<void> {
  const page = locator.page();
  await expect(async () => {
    const box = await locator.boundingBox();
    const viewport = page.viewportSize();
    expect(box, "element has no layout box (hidden or detached)").not.toBeNull();
    expect(viewport).not.toBeNull();
    const b = box!;
    const v = viewport!;
    expect(b.width, "element has zero width").toBeGreaterThan(0);
    expect(b.height, "element has zero height").toBeGreaterThan(0);
    // Sub-pixel rounding tolerance only.
    const eps = 0.5;
    const box4 = { left: b.x, top: b.y, right: b.x + b.width, bottom: b.y + b.height };
    const inside =
      box4.left >= -eps &&
      box4.top >= -eps &&
      box4.right <= v.width + eps &&
      box4.bottom <= v.height + eps;
    expect(
      inside,
      `element box ${JSON.stringify(box4)} must lie inside the ${v.width}×${v.height} viewport`,
    ).toBe(true);
  }).toPass({ timeout: 5_000 });
}

/**
 * The element is what a click at its centre would hit: no overlay, sibling
 * or ancestor clipping covers it. A child of the element counts as the
 * element itself.
 */
export async function expectTopmostAtCenter(locator: Locator): Promise<void> {
  await expect(async () => {
    const hit = await locator.evaluate((el) => {
      const r = el.getBoundingClientRect();
      const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      const describe = (n: Element | null) =>
        n ? `<${n.tagName.toLowerCase()} class="${n.getAttribute("class") ?? ""}">` : "nothing";
      return { ok: top !== null && (top === el || el.contains(top)), found: describe(top) };
    });
    expect(hit.ok, `element is covered at its centre by ${hit.found}`).toBe(true);
  }).toPass({ timeout: 5_000 });
}

/** Visible, inside the viewport and not covered. */
export async function expectUsable(locator: Locator): Promise<void> {
  await expect(locator).toBeVisible();
  await expectFullyInViewport(locator);
  await expectTopmostAtCenter(locator);
}

/**
 * The key that moves focus to the next control. WebKit on macOS follows
 * Safari's default and skips buttons on a plain Tab; Option+Tab is the
 * platform's "every control" variant. Elsewhere it is Tab.
 */
function tabKey(page: Page): string {
  const isMacWebKit = page.context().browser()?.browserType().name() === "webkit" && process.platform === "darwin";
  return isMacWebKit ? "Alt+Tab" : "Tab";
}

/**
 * Pressing Tab from the current focus reaches the element within
 * `maxPresses` steps. Returns the number of presses it took.
 */
export async function expectReachableByTab(
  page: Page,
  locator: Locator,
  { maxPresses = 40, backwards = false } = {},
): Promise<number> {
  const handle = await locator.elementHandle();
  expect(handle, "element to reach by keyboard is not in the DOM").not.toBeNull();
  for (let i = 1; i <= maxPresses; i++) {
    await page.keyboard.press(backwards ? `Shift+${tabKey(page)}` : tabKey(page));
    const focused = await handle!.evaluate((el) => el === document.activeElement);
    if (focused) return i;
  }
  const active = await page.evaluate(() => document.activeElement?.outerHTML.slice(0, 120) ?? "none");
  throw new Error(`element not reached by keyboard within ${maxPresses} Tab presses (focus ended on ${active})`);
}

/**
 * The nearest ancestor of `content` that scrolls vertically, searched only
 * up to `boundary` (a dialog). Fails if the content has no own scroll
 * container inside the boundary.
 */
export async function scrollContainerWithin(boundary: Locator, content: Locator): Promise<Locator> {
  const boundaryHandle = await boundary.elementHandle();
  expect(boundaryHandle, "dialog not found").not.toBeNull();
  const marker = `scroller-${Math.random().toString(36).slice(2)}`;
  const found = await content.evaluate(
    (el, [b, m]) => {
      for (let n = el.parentElement; n && n !== (b as Element).parentElement; n = n.parentElement) {
        const overflowY = getComputedStyle(n).overflowY;
        if (overflowY === "auto" || overflowY === "scroll") {
          n.setAttribute("data-e2e-scroller", m as string);
          return true;
        }
      }
      return false;
    },
    [boundaryHandle, marker] as const,
  );
  expect(found, "the content has no scroll container of its own inside the dialog").toBe(true);
  return content.page().locator(`[data-e2e-scroller="${marker}"]`);
}

/**
 * `scroller` scrolls its own content vertically (content taller than the
 * box), and scrolling it does not move `fixedParts` — header and buttons
 * stay where they were.
 */
export async function expectOnlyContentScrolls(
  scroller: Locator,
  fixedParts: Locator[],
): Promise<void> {
  const metrics = await scroller.evaluate((el) => ({
    scrollHeight: el.scrollHeight,
    clientHeight: el.clientHeight,
    overflowY: getComputedStyle(el).overflowY,
  }));
  expect(
    metrics.scrollHeight,
    `content area must overflow its box (overflow-y: ${metrics.overflowY})`,
  ).toBeGreaterThan(metrics.clientHeight);
  expect(["auto", "scroll"], "content area must be a scroll container").toContain(metrics.overflowY);

  const before = await Promise.all(fixedParts.map((p) => p.boundingBox()));
  await scroller.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  await expect.poll(() => scroller.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  const after = await Promise.all(fixedParts.map((p) => p.boundingBox()));
  expect(after, "header/buttons moved while the content scrolled").toEqual(before);
}
