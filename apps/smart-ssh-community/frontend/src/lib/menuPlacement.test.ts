// Issue #627
import { describe, expect, it } from "vitest";
import { type MenuRect, placeMenu } from "./menuPlacement";

const VP = { width: 1000, height: 800 };
const SIZE = { width: 200, height: 300 };
const point = (x: number, y: number): MenuRect => ({ left: x, top: y, right: x, bottom: y });
const place = (anchor: MenuRect, extra: object = {}) =>
  placeMenu({ anchor, size: SIZE, viewport: VP, margin: 8, ...extra });

describe("placeMenu", () => {
  it("opens below/right of the anchor when there is room", () => {
    expect(place(point(100, 100))).toEqual({ left: 100, top: 100, maxHeight: null });
  });
  it("flips up at the bottom edge", () => {
    expect(place(point(100, 780)).top).toBe(480);
  });
  it("flips left at the right edge", () => {
    expect(place(point(990, 100)).left).toBe(790);
  });
  it("flips both ways in the bottom right corner", () => {
    expect(place(point(995, 795))).toEqual({ left: 792, top: 495, maxHeight: null });
  });
  it("is kept at the window margin near the top and left edges", () => {
    expect(place(point(2, 2))).toEqual({ left: 8, top: 2, maxHeight: null });
  });
  it("end alignment lines up right edges and flips to start at the left edge", () => {
    const el: MenuRect = { left: 700, top: 10, right: 800, bottom: 30 };
    expect(place(el, { align: "end" }).left).toBe(600);
    const near: MenuRect = { left: 20, top: 10, right: 60, bottom: 30 };
    expect(place(near, { align: "end" }).left).toBe(20);
  });
  it("uses the gap and flips above the element", () => {
    const el: MenuRect = { left: 100, top: 700, right: 140, bottom: 720 };
    expect(place(el, { gap: 4 })).toEqual({ left: 100, top: 396, maxHeight: null });
  });
  it("shifts into the window when neither side has room", () => {
    const p = place(point(100, 400), { size: { width: 200, height: 700 } });
    expect(p.top).toBe(92);
    expect(p.maxHeight).toBeNull();
  });
  it("shifts horizontally when neither side fits", () => {
    const p = placeMenu({ anchor: point(150, 10), size: { width: 280, height: 50 }, viewport: { width: 300, height: 800 } });
    expect(p.left).toBe(12);
  });
  it("caps the height and scrolls when the menu is larger than the window", () => {
    const p = place(point(500, 400), { size: { width: 1200, height: 2000 } });
    expect(p).toEqual({ left: 8, top: 8, maxHeight: 784 });
  });
});
