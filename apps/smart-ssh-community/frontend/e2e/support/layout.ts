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
export function tabKey(page: Page): string {
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

/**
 * Tab order and focus indicators inside `scope`: the visible, focusable
 * controls are collected in DOM order; pressing Tab from the first one must
 * visit exactly these controls in this order, each with a visible focus
 * indicator (outline or ring), and that order must follow the visual order
 * (rows top to bottom, within a row left to right). A named radio group counts as one stop. Controls in a nested
 * `[role=dialog]` of the scope are ignored.
 */
export async function expectTabOrderFollowsVisualOrder(page: Page, scope: Locator): Promise<number> {
  const stops = await scope.evaluate((root) => {
    const selector =
      'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
    const all = Array.from(root.querySelectorAll<HTMLElement>(selector)).filter((el) => {
      const r = el.getBoundingClientRect();
      return el.tabIndex >= 0 && r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden";
    }).filter((el) => {
      // A radio group is one Tab stop (the checked radio); the arrow keys move within it.
      if (!(el instanceof HTMLInputElement) || el.type !== "radio" || el.checked || !el.name) return true;
      return !root.querySelector(`input[type=radio][name="${CSS.escape(el.name)}"]:checked`);
    });
    all.forEach((el, i) => el.setAttribute("data-e2e-stop", String(i)));
    return all.map((el, i) => {
      const r = el.getBoundingClientRect();
      return { index: i, top: r.top, left: r.left, bottom: r.bottom, name: `${el.tagName.toLowerCase()} "${(el.getAttribute("aria-label") ?? el.textContent ?? "").trim().slice(0, 40)}"` };
    });
  });
  expect(stops.length, "no focusable controls in scope").toBeGreaterThan(0);

  // Visual order: a stop may not sit clearly above its predecessor, and in
  // the same row (vertical overlap) not left of it.
  for (let i = 1; i < stops.length; i++) {
    const prev = stops[i - 1]!;
    const cur = stops[i]!;
    const sameRow = cur.top < prev.bottom - 2 && prev.top < cur.bottom - 2;
    const ok = sameRow ? cur.left >= prev.left - 1 : cur.top >= prev.top - 1;
    expect(ok, `Tab stop ${cur.name} comes after ${prev.name} but is placed before it`).toBe(true);
  }

  await scope.locator(`[data-e2e-stop="0"]`).focus();
  // Programmatic focus after a mouse click does not match :focus-visible;
  // one Tab forward and back puts the focus on stop 0 as the keyboard does.
  if (stops.length > 1) {
    await page.keyboard.press(tabKey(page));
    await page.keyboard.press(`Shift+${tabKey(page)}`);
  }
  for (let i = 0; i < stops.length; i++) {
    if (i > 0) await page.keyboard.press(tabKey(page));
    const focus = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el) return null;
      const cs = getComputedStyle(el);
      const outline = cs.outlineStyle !== "none" && parseFloat(cs.outlineWidth) > 0;
      const ring = cs.boxShadow !== "none" && cs.boxShadow !== "";
      return { stop: el.getAttribute("data-e2e-stop"), visible: outline || ring, html: el.outerHTML.slice(0, 80) + ` outline=${cs.outlineStyle}/${cs.outlineWidth} shadow=${cs.boxShadow} focusVisible=${el.matches(":focus-visible")} focus=${el.matches(":focus")}` };
    });
    expect(focus?.stop, `Tab press ${i} should reach stop ${i}`).toBe(String(i));
    expect(focus?.visible, `no visible focus indicator on ${focus?.html}`).toBe(true);
  }
  return stops.length;
}

/**
 * Nothing in `scope` sticks out of it sideways and no element cuts its own
 * text off (horizontal overflow). Vertical overflow is fine: the content
 * scrolls.
 */
export async function expectNoClippedText(scope: Locator): Promise<void> {
  const problems = await scope.evaluate((root) => {
    const box = root.getBoundingClientRect();
    const out: string[] = [];
    for (const el of Array.from(root.querySelectorAll<HTMLElement>("*"))) {
      const r = el.getBoundingClientRect();
      if (r.width === 0 || r.height === 0 || !el.textContent?.trim()) continue;
      const describe = `<${el.tagName.toLowerCase()}> "${el.textContent.trim().slice(0, 40)}"`;
      if (r.right > box.right + 1 || r.left < box.left - 1) out.push(`${describe} sticks out sideways`);
      const cs = getComputedStyle(el);
      if (
        cs.textOverflow === "ellipsis" ||
        cs.overflowX === "hidden"
      ) {
        if (el.scrollWidth > el.clientWidth + 1 && !el.hasAttribute("title")) {
          out.push(`${describe} is cut off without a title`);
        }
      }
    }
    return out;
  });
  expect(problems).toEqual([]);
}

/**
 * Walks the Tab order from the current focus and returns, for each stop, the
 * accessible name (aria-label, label text or text content) and whether the
 * focused element shows a focus indicator (outline or ring).
 */
export async function walkTabOrder(
  page: Page,
  stops: number,
): Promise<{ name: string; indicator: boolean }[]> {
  const result: { name: string; indicator: boolean }[] = [];
  for (let i = 0; i < stops; i++) {
    await page.keyboard.press(tabKey(page));
    result.push(
      await page.evaluate(() => {
        const el = document.activeElement as HTMLElement | null;
        if (!el) return { name: "none", indicator: false };
        const style = getComputedStyle(el);
        const labelled = el.getAttribute("aria-labelledby");
        const name =
          el.getAttribute("aria-label") ??
          (labelled ? document.getElementById(labelled)?.textContent : null) ??
          (el as HTMLInputElement).labels?.[0]?.textContent ??
          el.textContent ??
          "";
        const outline = style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0;
        return { name: name.trim(), indicator: outline || style.boxShadow !== "none" };
      }),
    );
  }
  return result;
}

/**
 * The vertical centres of both elements are the same within `tolerance` px
 * (default 2). Retries until it holds or times out.
 */
export async function expectVerticallyCentredWith(
  a: Locator,
  b: Locator,
  tolerance = 2,
): Promise<void> {
  await expect(async () => {
    const [boxA, boxB] = await Promise.all([a.boundingBox(), b.boundingBox()]);
    expect(boxA, "first element has no layout box").not.toBeNull();
    expect(boxB, "second element has no layout box").not.toBeNull();
    const centreA = boxA!.y + boxA!.height / 2;
    const centreB = boxB!.y + boxB!.height / 2;
    expect(
      Math.abs(centreA - centreB),
      `vertical centres differ: ${centreA} vs ${centreB}`,
    ).toBeLessThanOrEqual(tolerance);
  }).toPass({ timeout: 5_000 });
}
