import { describe, expect, it, vi } from "vitest";
import { createEventsMock } from "./eventsMock";

describe("createEventsMock", () => {
  it("mocks every on… function export of events.ts and keeps the rest real", async () => {
    const actual = await vi.importActual<Record<string, unknown>>("../api/events");
    const mock = (await createEventsMock()) as unknown as Record<string, unknown>;
    const listeners = Object.keys(actual).filter(
      (n) => typeof actual[n] === "function" && /^on[A-Z]/.test(n),
    );
    expect(listeners.length).toBeGreaterThan(20);
    for (const name of listeners) {
      expect(vi.isMockFunction(mock[name]), name).toBe(true);
    }
    expect(Object.keys(mock).sort()).toEqual(Object.keys(actual).sort());
    expect(mock.base64ToBytes).toBe(actual.base64ToBytes);
  });

  it("resolves a listener to a no-op unlisten function", async () => {
    const mock = await createEventsMock();
    const unlisten = await mock.onChatError(() => {});
    expect(unlisten()).toBeUndefined();
  });

  it("lets overrides win over the defaults", async () => {
    const custom = vi.fn(() => Promise.resolve(() => {}));
    const mock = await createEventsMock({ onChatError: custom as never });
    expect(mock.onChatError).toBe(custom);
    expect(vi.isMockFunction(mock.onChatTextDelta)).toBe(true);
  });
});
