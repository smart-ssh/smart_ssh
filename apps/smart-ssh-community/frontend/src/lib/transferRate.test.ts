import { describe, expect, it } from "vitest";
import { formatDuration, formatSpeed, nextRate, type RateState } from "./transferRate";

const MB = 1024 * 1024;

function run(events: [number, number][], total: number | null): RateState {
  let st: RateState | undefined;
  for (const [t, b] of events) st = nextRate(st, t, b, total);
  return st as RateState;
}

describe("nextRate", () => {
  it("computes speed and remaining time as a moving average", () => {
    const st = run([[0, 0], [1000, MB], [2000, 2 * MB]], 10 * MB);
    expect(st.speed).toBe(MB);
    expect(st.etaSeconds).toBe(8);
  });

  it("drops samples older than the window", () => {
    // Fast start, then 1 MB/s: after 5 s the fast part no longer counts.
    const st = run([[0, 0], [500, 50 * MB], [6000, 55 * MB], [7000, 56 * MB]], 100 * MB);
    expect(st.speed).toBeCloseTo(MB, 0);
  });

  it("shows no remaining time for unknown total or zero speed", () => {
    expect(run([[0, 0], [1000, MB]], null).etaSeconds).toBeNull();
    expect(run([[0, MB], [1000, MB]], 10 * MB).etaSeconds).toBeNull();
    expect(run([[0, 0]], 10 * MB).speed).toBeNull();
  });

  it("refreshes the shown values at most twice per second", () => {
    const a = run([[0, 0], [1000, MB]], 10 * MB);
    const b = nextRate(a, 1200, 2 * MB, 10 * MB);
    expect(b.speed).toBe(a.speed);
    expect(b.shownAt).toBe(a.shownAt);
    const c = nextRate(b, 1500, 3 * MB, 10 * MB);
    expect(c.shownAt).toBe(1500);
    expect(c.speed).toBe(2 * MB);
  });
});

describe("formatters", () => {
  it("formats speed", () => {
    expect(formatSpeed(512 * 1024)).toBe("512.0 KB/s");
    expect(formatSpeed(2.5 * MB)).toBe("2.5 MB/s");
    expect(formatSpeed(null)).toBe("—");
  });
  it("formats durations", () => {
    expect(formatDuration(65)).toBe("01:05");
    expect(formatDuration(3725)).toBe("1:02:05");
    expect(formatDuration(null)).toBe("—");
  });
});
