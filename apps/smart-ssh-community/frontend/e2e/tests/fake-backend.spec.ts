// The fake backend itself: a command it does not know is rejected with a
// clear error and recorded, so the `app` fixture fails the test.
import { expect, test } from "../support/app";
import { EMPTY } from "../support/data";

test("an unknown command is rejected with a clear error and recorded", async ({ app }) => {
  await app.launch(EMPTY);
  const outcome = await app.page.evaluate(async () => {
    const internals = (window as unknown as {
      __TAURI_INTERNALS__: { invoke(cmd: string, args: unknown): Promise<unknown> };
    }).__TAURI_INTERNALS__;
    try {
      const value = await internals.invoke("definitely_not_a_command", { x: 1 });
      return { rejected: false, value };
    } catch (err) {
      return { rejected: true, err };
    }
  });
  expect(outcome).toEqual({
    rejected: true,
    err: {
      message: '[e2e fake backend] unknown command "definitely_not_a_command" — add a handler or a per-test override',
      code: "E2E_UNKNOWN_COMMAND",
    },
  });
  expect(await app.page.evaluate(() => window.__e2e!.unknownCommands)).toEqual(["definitely_not_a_command"]);
  expect(app.consoleErrors.some((e) => e.includes('unknown command "definitely_not_a_command"'))).toBe(true);

  // This test provoked the unknown command on purpose; clear the record so
  // the fixture's after-test check (which would fail it) passes.
  await app.page.evaluate(() => window.__e2e!.unknownCommands.splice(0));
});
