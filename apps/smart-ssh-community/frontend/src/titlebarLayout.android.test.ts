import { describe, expect, it } from "vitest";
import {
  detectFallbackPlatform,
  platformCapabilities,
  titlebarDragInsetStyle,
  titlebarPaddingStyle,
} from "./titlebarLayout";

describe("Android (Spec 0114)", () => {
  it("reserves no window-control space, even while the overlay is pending", () => {
    for (const mode of ["pending", "custom", "native"] as const) {
      expect(titlebarPaddingStyle("android", mode)).toEqual({
        paddingLeft: "16px",
        paddingRight: "16px",
      });
      expect(titlebarDragInsetStyle("android", mode)).toEqual({ left: "0px", right: "0px" });
    }
  });

  it("hides local server, folder picker and file clipboard and forces the narrow layout", () => {
    expect(platformCapabilities("android")).toEqual({
      localServer: false,
      folderPicker: false,
      fileClipboard: false,
      alwaysNarrow: true,
    });
  });

  it("keeps everything on desktop", () => {
    for (const p of ["macos", "windows", "linux", "unknown"] as const) {
      expect(platformCapabilities(p)).toEqual({
        localServer: true,
        folderPicker: true,
        fileClipboard: true,
        alwaysNarrow: false,
      });
    }
  });

  it("detects Android before Linux from the user agent", () => {
    const original = navigator.userAgent;
    Object.defineProperty(navigator, "userAgent", {
      value: "Mozilla/5.0 (Linux; Android 14; Pixel) AppleWebKit/537.36",
      configurable: true,
    });
    try {
      expect(detectFallbackPlatform()).toBe("android");
    } finally {
      Object.defineProperty(navigator, "userAgent", { value: original, configurable: true });
    }
  });
});
