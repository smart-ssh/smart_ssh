import { describe, expect, it, vi } from "vitest";
import { checkStartDirectory, notifyMissingStartDirectory } from "./startDirectory";
import { testI18n } from "./testI18n";
import { showToast } from "./toastBus";

vi.mock("./toastBus", () => ({ showToast: vi.fn() }));

describe("checkStartDirectory", () => {
  it("treats empty and blank input as not set", () => {
    expect(checkStartDirectory("")).toEqual({ ok: true, value: null });
    expect(checkStartDirectory("   ")).toEqual({ ok: true, value: null });
  });

  it("accepts absolute and home-relative paths, trimmed", () => {
    expect(checkStartDirectory("  /srv/my app ")).toEqual({ ok: true, value: "/srv/my app" });
    expect(checkStartDirectory("/")).toEqual({ ok: true, value: "/" });
    expect(checkStartDirectory("~/it's here")).toEqual({ ok: true, value: "~/it's here" });
    expect(checkStartDirectory("~/")).toEqual({ ok: true, value: "~/" });
  });

  it("rejects relative paths other than ~/…", () => {
    for (const bad of ["srv", "./srv", "../srv", "~", "~root/x", "~x"]) {
      expect(checkStartDirectory(bad)).toEqual({ ok: false, reason: "notAbsolute" });
    }
  });

  it("rejects control characters", () => {
    expect(checkStartDirectory("/srv/a\nb")).toEqual({ ok: false, reason: "controlCharacter" });
    expect(checkStartDirectory("/srv/\u001b[31m")).toEqual({
      ok: false,
      reason: "controlCharacter",
    });
  });
});

describe("notifyMissingStartDirectory", () => {
  it("shows one notice naming the directory", () => {
    notifyMissingStartDirectory(testI18n.t, "/srv/gone");
    expect(showToast).toHaveBeenCalledTimes(1);
    expect(vi.mocked(showToast).mock.calls[0][0].message).toContain("/srv/gone");
  });

  it("shows nothing without a missing directory", () => {
    vi.mocked(showToast).mockClear();
    notifyMissingStartDirectory(testI18n.t, null);
    expect(showToast).not.toHaveBeenCalled();
  });
});
