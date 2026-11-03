import { describe, expect, it, vi } from "vitest";
import { createApiMock } from "./apiMock";

describe("createApiMock", () => {
  it("mocks every function export of api.ts except the pure error helpers", async () => {
    const actual = await vi.importActual<Record<string, unknown>>("../api");
    const mock = (await createApiMock()) as unknown as Record<string, unknown>;
    const pure = new Set(["commandErrorMessage", "commandErrorCode"]);
    const names = Object.keys(actual).filter(
      (n) => typeof actual[n] === "function" && !pure.has(n),
    );
    expect(names.length).toBeGreaterThan(50);
    for (const name of names) {
      expect(vi.isMockFunction(mock[name]), name).toBe(true);
    }
    expect(Object.keys(mock).sort()).toEqual(Object.keys(actual).sort());
  });

  it("keeps the real error helpers", async () => {
    const mock = await createApiMock();
    expect(mock.commandErrorMessage({ message: "boom" })).toBe("boom");
    expect(mock.commandErrorCode({ code: "X" })).toBe("X");
  });

  it("lets overrides win over the defaults", async () => {
    const custom = vi.fn(() => Promise.resolve([{ id: "x" }]));
    const mock = await createApiMock({ getChatHistory: custom as never });
    expect(mock.getChatHistory).toBe(custom);
    expect(await mock.listPromptHistory("s")).toEqual([]);
  });
});
