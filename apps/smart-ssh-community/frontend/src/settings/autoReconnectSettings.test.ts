import { describe, expect, it } from "vitest";
import { parseAutoReconnectSeconds } from "./autoReconnectSettings";

describe("parseAutoReconnectSeconds", () => {
  it("defaults to 60 s when the key is missing", () => {
    expect(parseAutoReconnectSeconds(undefined)).toBe(60);
    expect(parseAutoReconnectSeconds(null)).toBe(60);
  });

  it("accepts exactly Off, 30 s, 60 s and 5 min", () => {
    expect([0, 30, 60, 300].map(parseAutoReconnectSeconds)).toEqual([0, 30, 60, 300]);
  });

  it("falls back to the default for anything else, never to unlimited", () => {
    expect(parseAutoReconnectSeconds(99999)).toBe(60);
    expect(parseAutoReconnectSeconds("off")).toBe(60);
    expect(parseAutoReconnectSeconds(-1)).toBe(60);
  });
});
