// Shared auto-mock for `../events` (issue #1112).
//
// Every `on…` listener export of `events.ts` becomes
// `vi.fn(() => Promise.resolve(() => {}))` (a subscription that never fires and
// whose unlisten is a no-op); every other export keeps its real value. A new
// listener in `events.ts` is therefore mocked automatically — no per-file
// factory to extend.
//
// Usage — one factory per test file (path relative to the test file):
//
//   vi.mock("../api/events", async () => (await import("../test/eventsMock")).createEventsMock());
//   vi.mock("../api/events", async () =>
//     (await import("../test/eventsMock")).createEventsMock({ onChatTextDelta: vi.fn(...) }));
//
// Overrides win over the defaults. ADR 0122 R9: nothing in here waits.
import { vi } from "vitest";

export type EventsModule = typeof import("../api/events");

export async function createEventsMock(
  overrides: Partial<EventsModule> = {},
): Promise<EventsModule> {
  const actual = await vi.importActual<EventsModule>("../api/events");
  const mock: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(actual)) {
    mock[name] =
      typeof value === "function" && /^on[A-Z]/.test(name)
        ? vi.fn(() => Promise.resolve(() => {}))
        : value;
  }
  return { ...mock, ...overrides } as EventsModule;
}
