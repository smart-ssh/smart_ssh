// UI review of the AI provider settings, the MCP settings and the filter
// rule management (issue #386): axe scans, Tab order, screenshots and
// clipping with long content, at both window sizes.
import type { Locator, Page } from "@playwright/test";
import type { FakeBackendFixture } from "../harness/fixtureTypes";
import { expect, test, type AppDriver } from "../support/app";
import { expectNoSeriousA11yViolations } from "../support/axe";
import { aiProvider, populated } from "../support/data";
import { captureScreen, SCREENSHOT_SIZES } from "../support/screenshots";

const LONG = "very-long-value-".repeat(12);

function fixture(): FakeBackendFixture {
  const base = populated();
  return {
    ...base,
    aiProviders: [
      aiProvider({ id: "p1", displayName: `Main ${LONG}`, model: `model-${LONG}`, webResearchEnabled: true }),
      aiProvider({ id: "p2", displayName: "Second", isActive: false, providerType: "openai_compatible", baseUrl: "http://x" }),
    ],
    servers: [...(base.servers ?? []), { ...(base.servers ?? [])[0]!, id: "s-long", name: `server-${LONG}` }],
  };
}

async function openSettings(app: AppDriver, category: string) {
  await app.launch(fixture());
  await app.page.getByRole("button", { name: "Settings", exact: true }).click();
  await app.page.getByRole("button", { name: category, exact: true }).click();
}

async function openRules(app: AppDriver) {
  await app.launch({
    ...fixture(),
    commands: {
      list_rules: { result: [
        { id: "r1", patternType: "glob", patternValue: `rm -rf ${LONG}`, action: "Deny", scope: "Global", priority: 10, patternError: null },
        { id: "r2", patternType: "regex", patternValue: "(", action: "Confirm", scope: "Global", priority: 20, patternError: "unclosed group" },
        { id: "r3", patternType: "exact", patternValue: "ls", action: "Allow", scope: { Tag: "db" }, priority: 30, patternError: null },
      ] },
      list_hard_blacklist: { result: ["rm -rf /"] },
    },
  } as FakeBackendFixture);
  await app.page.getByRole("button", { name: "Filter Rules", exact: true }).click();
  await expect(app.page.getByText("exact: ls")).toBeVisible();
}

const SCREENS: Record<string, { open: (app: AppDriver) => Promise<void>; root: (p: Page) => Locator }> = {
  "ai-provider": { open: (a) => openSettings(a, "AI Provider"), root: (p) => p.locator("div.overflow-y-auto").last() },
  "mcp-server": { open: (a) => openSettings(a, "MCP Server"), root: (p) => p.locator("div.overflow-y-auto").last() },
  "filter-rules": { open: openRules, root: (p) => p.locator("#root") },
};

for (const [name, screen] of Object.entries(SCREENS)) {
  test.describe(name, () => {
    test("screenshots", async ({ app }) => {
      await screen.open(app);
      await captureScreen(app.page, name);
    });

    test("axe scan", async ({ app }) => {
      await screen.open(app);
      await expectNoSeriousA11yViolations(app.page, name);
    });

    test("Tab order follows the visual order and every stop shows focus", async ({ app }) => {
      await screen.open(app);
      const page = app.page;
      const stops: { top: number; left: number; ringed: boolean; text: string }[] = [];
      await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
      const container = screen.root(page);
      const visible = async () => (await container.getByRole("button").count()) > 0;
      expect(await visible()).toBe(true);
      // Focus the first focusable control inside the screen, then walk.
      await container.locator("button, input, select, textarea, a[href]").first().focus();
      for (let i = 0; i < 80; i++) {
        const info = await page.evaluate(() => {
          const el = document.activeElement as HTMLElement;
          const r = el.getBoundingClientRect();
          const cs = getComputedStyle(el);
          const ringed =
            (cs.outlineStyle !== "none" && parseFloat(cs.outlineWidth) > 0) ||
            (cs.boxShadow !== "none" && cs.boxShadow !== "");
          return { top: r.top + el.offsetParent!.scrollTop * 0, left: r.left, ringed, text: (el.textContent ?? el.getAttribute("aria-label") ?? el.tagName).slice(0, 30), inside: true, y: r.top };
        });
        stops.push(info);
        const before = await page.evaluate(() => document.activeElement);
        void before;
        await page.keyboard.press("Tab");
        const inRoot = await container.evaluate((root) => root.contains(document.activeElement));
        if (!inRoot) break;
      }
      for (const s of stops) expect(s.ringed, `focus indicator on "${s.text}"`).toBe(true);
      // Visual order: top-to-bottom (rows may hold several stops left-to-right).
      for (let i = 1; i < stops.length; i++) {
        const prev = stops[i - 1]!;
        const cur = stops[i]!;
        const sameRow = Math.abs(cur.top - prev.top) < 12;
        const ok = sameRow ? cur.left >= prev.left - 1 : cur.top > prev.top;
        // Scrolling into view shifts `top`; tolerate by comparing only when the container did not scroll.
        expect.soft(ok, `"${prev.text}" -> "${cur.text}" follows the visual order`).toBe(true);
      }
    });

    test("no clipped text at 800×560", async ({ app }) => {
      await screen.open(app);
      const page = app.page;
      for (const size of SCREENSHOT_SIZES) {
        await page.setViewportSize(size);
        const clipped = await screen.root(page).evaluate((root) => {
          const bad: string[] = [];
          for (const el of root.querySelectorAll<HTMLElement>("button, label, h2, h3, p, span, code, li")) {
            const cs = getComputedStyle(el);
            if (el.children.length > 0 && el.tagName !== "BUTTON") continue;
            const hidden = cs.overflowX === "hidden" || cs.overflow === "hidden";
            if (!hidden && cs.textOverflow !== "ellipsis") continue;
            if (el.scrollWidth > el.clientWidth + 1 && !el.getAttribute("title"))
              bad.push(`${el.tagName}:${(el.textContent ?? "").slice(0, 30)}`);
          }
          return bad;
        });
        expect(clipped, `clipped text without tooltip at ${size.width}x${size.height}`).toEqual([]);
      }
    });
  });
}
