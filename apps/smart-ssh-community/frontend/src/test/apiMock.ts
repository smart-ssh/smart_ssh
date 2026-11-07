// Shared auto-mock for `../api/api` (issue #906).
//
// Every function export of `api.ts` becomes a `vi.fn()` and every other export
// keeps its real value, so a new export in `api.ts` is mocked automatically —
// no per-file factory to extend. The real `commandErrorMessage` /
// `commandErrorCode` helpers are kept (they are pure).
//
// Usage — one factory per test file (path relative to the test file):
//
//   vi.mock("../api/api", async () => (await import("../test/apiMock")).createApiMock());
//   vi.mock("../api/api", async () =>
//     (await import("../test/apiMock")).createApiMock({ listAiProviders: vi.fn(() => Promise.resolve([])) }));
//
// Overrides win over the defaults below. A test that asserts a failing call
// sets that call explicitly (`vi.mocked(sendChatMessage).mockRejectedValue(…)`);
// the defaults only cover the success path of the calls every screen makes
// while mounting.
//
// ADR 0122 R9: nothing in here waits; the shared async upper bound is the
// `asyncUtilTimeout` in `test-setup.ts`.
import { vi } from "vitest";

export type ApiModule = typeof import("../api/api");

/** Success-path defaults for calls that screens make on mount or on a click. */
function defaultImplementations(): Partial<Record<keyof ApiModule, unknown>> {
  return {
    getChatHistory: vi.fn(() => Promise.resolve([])),
    sftpElevationAllowed: vi.fn(() => Promise.resolve(true)),
    listAiProviders: vi.fn(() => Promise.resolve([])),
    listPromptHistory: vi.fn(() => Promise.resolve([])),
    startNewChat: vi.fn(() => Promise.resolve("new-chat-id")),
    explainConfirmation: vi.fn(() =>
      Promise.resolve({ segments: [], suggestions: [] }),
    ),
    continueTruncatedResponse: vi.fn(() => Promise.resolve()),
    respondToAction: vi.fn(() => Promise.resolve()),
    sendChatMessage: vi.fn(() => Promise.resolve()),
    stopAutoContinuation: vi.fn(() => Promise.resolve()),
  };
}

export async function createApiMock(
  overrides: Partial<ApiModule> = {},
): Promise<ApiModule> {
  const actual = await vi.importActual<ApiModule>("../api/api");
  const defaults = defaultImplementations();
  const mock: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(actual)) {
    if (typeof value !== "function") {
      mock[name] = value;
    } else if (
      name === "commandErrorMessage" ||
      name === "commandErrorCode" ||
      name === "commandErrorConnectLog"
    ) {
      mock[name] = value;
    } else {
      mock[name] =
        name in defaults ? defaults[name as keyof ApiModule] : vi.fn();
    }
  }
  return { ...mock, ...overrides } as ApiModule;
}
