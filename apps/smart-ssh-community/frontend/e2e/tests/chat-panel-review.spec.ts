// Issue #383: chat panel review. Screenshots for the before/after images in
// the PR, Tab order in visual order with a visible focus indicator, Escape and
// focus return for the quick-rule popup, and no clipped text at 800×560.
import type { Page } from "@playwright/test";
import { expect, test } from "../support/app";
import { openChatWithConfirmation, openChatWithMessages } from "../support/chatScreens";
import { expectFullyInViewport } from "../support/layout";
import { captureScreen } from "../support/screenshots";
import { chatInput, redCommandProposal } from "../support/session";

const SIZES = [
  { width: 1280, height: 800 },
  { width: 800, height: 560 },
];

/** Presses Tab `count` times from the current focus and returns, for every
 * stop, the element's accessible description and whether it shows a focus
 * indicator (outline or box-shadow ring). */
async function tabStops(page: Page, count: number) {
  const stops: { name: string; top: number; left: number; ring: boolean }[] = [];
  for (let i = 0; i < count; i++) {
    await page.keyboard.press("Tab");
    stops.push(
      await page.evaluate(() => {
        const el = document.activeElement as HTMLElement;
        // Position inside the scrolling chat column, so scrolling a focused
        // element into view does not change the order.
        let scroller: HTMLElement | null = el.parentElement;
        while (scroller && !["auto", "scroll"].includes(getComputedStyle(scroller).overflowY)) {
          scroller = scroller.parentElement;
        }
        const r = el.getBoundingClientRect();
        const offset = scroller ? scroller.scrollTop - scroller.getBoundingClientRect().top : 0;
        const cs = getComputedStyle(el);
        const outline = cs.outlineStyle !== "none" && parseFloat(cs.outlineWidth) > 0;
        const shadow = cs.boxShadow !== "none" && cs.boxShadow !== "";
        return {
          name: el.getAttribute("aria-label") ?? (el.innerText || el.tagName).trim().slice(0, 40),
          top: Math.round(r.top + offset),
          left: Math.round(r.left),
          ring: outline || shadow,
        };
      }),
    );
  }
  return stops;
}

function expectVisualOrder(stops: { name: string; top: number; left: number }[]) {
  // Reading order: a stop never lies clearly above the previous one; within a
  // row it moves right.
  for (let i = 1; i < stops.length; i++) {
    const a = stops[i - 1];
    const b = stops[i];
    const sameRow = Math.abs(a.top - b.top) < 8;
    const ok = sameRow ? b.left >= a.left : b.top > a.top;
    expect(ok, `Tab stop "${b.name}" (${b.left},${b.top}) breaks the visual order after "${a.name}" (${a.left},${a.top})`).toBe(true);
  }
}

test.describe("chat panel review", () => {
  test("screenshots of the chat states", async ({ app }) => {
    await openChatWithMessages(app);
    await captureScreen(app.page, "chat-messages");
  });

  test("screenshot of the command confirmation and its quick-rule popup", async ({ app }) => {
    await openChatWithConfirmation(app);
    await captureScreen(app.page, "chat-confirmation");
    await app.page.getByRole("button", { name: "Show rule suggestions" }).click();
    await expect(app.page.getByText("rm -rf /var/lib/app/*")).toBeVisible();
    await captureScreen(app.page, "chat-quick-rule");
  });

  test("Tab order of the confirmation follows the visual order, every stop has a focus indicator", async ({ app }) => {
    await openChatWithConfirmation(app);
    const editor = app.page.locator("textarea").filter({ hasText: "rm -rf" }).locator("visible=true");
    await editor.focus();
    const stops = await tabStops(app.page, 4);
    expect(stops.map((s) => s.name.split("\n")[0])).toEqual(
      expect.arrayContaining(["EXECUTE", "DENY"].map((n) => expect.stringMatching(new RegExp(n, "i")))),
    );
    expectVisualOrder(stops);
    for (const s of stops) expect(s.ring, `no focus indicator on "${s.name}"`).toBe(true);
  });

  test("Tab order of the message area and the input", async ({ app }) => {
    await openChatWithMessages(app);
    await app.page.getByRole("button", { name: "New chat" }).focus();
    const stops = await tabStops(app.page, 6);
    expectVisualOrder(stops);
    for (const s of stops) expect(s.ring, `no focus indicator on "${s.name}"`).toBe(true);
    const input = chatInput(app.page);
    await input.focus();
    const ring = await input.evaluate((el) => {
      const cs = getComputedStyle(el);
      return cs.boxShadow !== "none" || (cs.outlineStyle !== "none" && parseFloat(cs.outlineWidth) > 0);
    });
    expect(ring, "the chat input shows no focus indicator").toBe(true);
  });

  test("the quick-rule popup closes with Escape and focus returns to its trigger", async ({ app }) => {
    await openChatWithConfirmation(app);
    const trigger = app.page.getByRole("button", { name: "Show rule suggestions" });
    await trigger.click();
    await expect(app.page.getByText("rm -rf /var/lib/app/*")).toBeVisible();
    await app.page.keyboard.press("Escape");
    await expect(app.page.getByText("rm -rf /var/lib/app/*")).toHaveCount(0);
    await expect(trigger).toBeFocused();
  });

  for (const size of SIZES) {
    test(`no clipped text in the chat at ${size.width}×${size.height}`, async ({ app }) => {
      await app.page.setViewportSize(size);
      await openChatWithMessages(app);
      const sessionId = await app.page.evaluate(
        () => (window.__e2e!.snapshot() as { sessions: { sessionId: string }[] }).sessions[0].sessionId,
      );
      await app.emit("chat-action-proposed", redCommandProposal(sessionId, `echo ${"x".repeat(300)}`));
      await expect(app.page.getByRole("button", { name: "Execute", exact: true })).toBeAttached();
      // No element in the chat column is wider than its scroll container
      // (nothing sticks out or is cut off horizontally).
      const overflowing: string[] = await app.page.evaluate(() => {
        const root = document.querySelector("div.overflow-y-auto.flex-1") as HTMLElement | null;
        if (!root) return ["no scroll container"];
        const box = root.getBoundingClientRect();
        const bad: string[] = [];
        for (const el of Array.from(root.querySelectorAll("*")) as HTMLElement[]) {
          const r = el.getBoundingClientRect();
          if (r.width > 0 && r.right > box.right + 1) bad.push(`${el.tagName} ${(el.textContent ?? "").slice(0, 30)}`);
        }
        return bad;
      });
      expect(overflowing).toEqual([]);
      await expectFullyInViewport(chatInput(app.page));
    });
  }
});
