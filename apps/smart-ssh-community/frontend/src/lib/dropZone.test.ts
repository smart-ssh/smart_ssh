import { afterEach, describe, expect, it, vi } from "vitest";
import { isPointerInside, toCssPosition } from "./dropZone";

describe("toCssPosition", () => {
  it("physical scale 1 and 2", () => {
    expect(toCssPosition({ x: 800, y: 400 }, 1, "physical")).toEqual({
      x: 800,
      y: 400,
    });
    expect(toCssPosition({ x: 800, y: 400 }, 2, "physical")).toEqual({
      x: 400,
      y: 200,
    });
  });
  it("logical is left unchanged at any scale", () => {
    expect(toCssPosition({ x: 800, y: 400 }, 1, "logical")).toEqual({
      x: 800,
      y: 400,
    });
    expect(toCssPosition({ x: 800, y: 400 }, 2, "logical")).toEqual({
      x: 800,
      y: 400,
    });
  });
});

function zone(left: number, right: number): HTMLElement {
  return {
    offsetParent: {},
    getBoundingClientRect: () => ({ left, right, top: 0, bottom: 600 }),
  } as unknown as HTMLElement;
}

describe("isPointerInside at scale 2", () => {
  afterEach(() => vi.unstubAllGlobals());
  const chat = zone(0, 500);
  const files = zone(500, 1000);

  it("macOS (logical): point over the right-hand file browser", () => {
    vi.stubGlobal("navigator", {
      platform: "MacIntel",
      userAgent: "Macintosh",
    });
    vi.stubGlobal("window", { devicePixelRatio: 2 });
    const p = { x: 750, y: 100 };
    expect(isPointerInside(files, p)).toBe(true);
    expect(isPointerInside(chat, p)).toBe(false);
    const q = { x: 200, y: 100 };
    expect(isPointerInside(chat, q)).toBe(true);
    expect(isPointerInside(files, q)).toBe(false);
  });

  it("Windows (physical): same layout", () => {
    vi.stubGlobal("navigator", { platform: "Win32", userAgent: "Windows" });
    vi.stubGlobal("window", { devicePixelRatio: 2 });
    const p = { x: 1500, y: 200 };
    expect(isPointerInside(files, p)).toBe(true);
    expect(isPointerInside(chat, p)).toBe(false);
    const q = { x: 400, y: 200 };
    expect(isPointerInside(chat, q)).toBe(true);
    expect(isPointerInside(files, q)).toBe(false);
  });
});
