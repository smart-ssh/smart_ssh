// Playwright fixture that boots the real frontend against the fake backend.
//
// Every test gets an `app` object: `app.launch(fixture)` opens the harness
// page with that backend state, `app.calls(cmd)` reads what the frontend
// invoked, `app.emit(event, payload)` plays a backend event. After each
// test the fixture fails it if the app called a command the fake backend
// does not know.
import { expect, test as base, type Page } from "@playwright/test";
import type {
  CannedResponse,
  FakeBackendFixture,
  RecordedCall,
} from "../harness/fixtureTypes";

export const HARNESS_URL = "/e2e/harness/index.html";

export interface AppDriver {
  page: Page;
  /** Opens the app with this backend state and waits for the first screen. */
  launch(fixture?: FakeBackendFixture): Promise<void>;
  /** Reloads the page; the fake backend keeps its state (`sessionStorage`). */
  reload(): Promise<void>;
  /** Calls of `cmd` (or all calls) the frontend made since the last load. */
  calls(cmd?: string): Promise<RecordedCall[]>;
  /** Waits until a listener for `event` exists, then emits it once. */
  emit(event: string, payload: unknown): Promise<void>;
  setCommand(cmd: string, response: CannedResponse): Promise<void>;
  /** Waits for a held call of `cmd`, then settles it. */
  release(cmd: string, response: { result: unknown } | { error: unknown }): Promise<void>;
  /** Console errors and uncaught page errors since the page was opened. */
  consoleErrors: string[];
}

export const test = base.extend<{ app: AppDriver }>({
  app: async ({ page }, provide) => {
    const consoleErrors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") consoleErrors.push(msg.text());
    });
    page.on("pageerror", (err) => consoleErrors.push(`pageerror: ${err.message}`));

    const waitForApp = async () => {
      // `main.tsx` renders only after its start-up `invoke`s resolved; the
      // root then has content (main screen, or a dialog over it).
      await expect(page.locator("#root")).not.toBeEmpty({ timeout: 20_000 });
    };

    const driver: AppDriver = {
      page,
      consoleErrors,
      launch: async (fixture = {}) => {
        await page.addInitScript((f) => {
          window.__E2E_FIXTURE__ = f;
        }, fixture);
        await page.goto(HARNESS_URL);
        await waitForApp();
      },
      reload: async () => {
        await page.reload();
        await waitForApp();
      },
      calls: async (cmd) => {
        const all = await page.evaluate(() => window.__e2e!.calls);
        return cmd ? all.filter((c) => c.cmd === cmd) : all;
      },
      emit: async (event, payload) => {
        await expect
          .poll(() => page.evaluate((e) => window.__e2e!.listenerCount(e), event), {
            message: `no listener for "${event}"`,
          })
          .toBeGreaterThan(0);
        await page.evaluate(([e, p]) => window.__e2e!.emit(e, p), [event, payload] as const);
      },
      setCommand: async (cmd, response) => {
        await page.evaluate(([c, r]) => window.__e2e!.setCommand(c, r), [cmd, response] as const);
      },
      release: async (cmd, response) => {
        await expect
          .poll(() => page.evaluate((c) => window.__e2e!.heldCount(c), cmd), {
            message: `no held call of "${cmd}"`,
          })
          .toBeGreaterThan(0);
        await page.evaluate(([c, r]) => window.__e2e!.release(c, r), [cmd, response] as const);
      },
    };

    await provide(driver);

    // The fake backend rejects unknown commands, but the app may swallow
    // that rejection (many calls only log). Fail the test here instead.
    const unknown = page.isClosed()
      ? []
      : await page.evaluate(() => window.__e2e?.unknownCommands ?? []).catch(() => []);
    expect(unknown, "the app invoked commands the fake backend does not know").toEqual([]);
  },
});

export { expect };
