// Issue #915: `titlebarLayout.ts` keeps re-exporting the names that moved to `platform.ts`.
import { describe, expect, expectTypeOf, it } from "vitest";
import * as platform from "./platform";
import * as titlebar from "./titlebarLayout";
import type { Platform, PlatformCapabilities } from "./titlebarLayout";

describe("titlebarLayout re-exports", () => {
  it("re-exports the moved names", () => {
    expect(titlebar.detectFallbackPlatform).toBe(platform.detectFallbackPlatform);
    expect(titlebar.platformCapabilities).toBe(platform.platformCapabilities);
    expect(titlebar.usePlatform).toBe(platform.usePlatform);
    expectTypeOf<Platform>().toEqualTypeOf<platform.Platform>();
    expectTypeOf<PlatformCapabilities>().toEqualTypeOf<platform.PlatformCapabilities>();
  });
});
