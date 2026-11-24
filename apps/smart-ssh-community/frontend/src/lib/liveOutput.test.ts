import { describe, expect, it } from "vitest";
import { appendLiveOutput, LIVE_OUTPUT_TAIL_CHARS } from "./liveOutput";

describe("appendLiveOutput", () => {
  it("appends stdout and stderr separately", () => {
    const first = appendLiveOutput(undefined, { stdout: "1\n", stderr: "", truncated: false });
    const second = appendLiveOutput(first, { stdout: "2\n", stderr: "warn\n", truncated: false });
    expect(second).toEqual({ stdout: "1\n2\n", stderr: "warn\n", truncated: false });
  });

  it("keeps the truncated flag once set", () => {
    const truncated = appendLiveOutput(undefined, { stdout: "", stderr: "", truncated: true });
    const later = appendLiveOutput(truncated, { stdout: "x\n", stderr: "", truncated: false });
    expect(later.truncated).toBe(true);
  });

  it("keeps only the tail of a long stream, starting at a line start", () => {
    const line = "0123456789\n";
    const many = line.repeat(Math.ceil(LIVE_OUTPUT_TAIL_CHARS / line.length) + 50);
    const result = appendLiveOutput(undefined, { stdout: many, stderr: "", truncated: false });
    expect(result.stdout.length).toBeLessThanOrEqual(LIVE_OUTPUT_TAIL_CHARS);
    expect(result.stdout.startsWith("0123456789\n")).toBe(true);
    expect(result.stdout.endsWith(line)).toBe(true);
  });
});
