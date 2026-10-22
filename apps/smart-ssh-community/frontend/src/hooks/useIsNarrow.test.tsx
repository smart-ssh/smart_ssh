import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { installMatchMediaMock, removeMatchMedia, setViewportWidth, setWide } from "../test/matchMedia";
import { useIsNarrow } from "./useIsNarrow";

afterEach(() => {
  installMatchMediaMock();
  setWide();
});

describe("useIsNarrow", () => {
  it("is true below 640 px and false from 640 px", () => {
    setViewportWidth(639);
    expect(renderHook(() => useIsNarrow()).result.current).toBe(true);
    setViewportWidth(640);
    expect(renderHook(() => useIsNarrow()).result.current).toBe(false);
  });

  it("switches live when the window crosses the breakpoint", () => {
    setViewportWidth(1000);
    const { result } = renderHook(() => useIsNarrow());
    expect(result.current).toBe(false);
    act(() => setViewportWidth(390));
    expect(result.current).toBe(true);
    act(() => setViewportWidth(640));
    expect(result.current).toBe(false);
  });

  it("returns false without matchMedia and does not throw", () => {
    removeMatchMedia();
    expect(renderHook(() => useIsNarrow()).result.current).toBe(false);
  });
});
