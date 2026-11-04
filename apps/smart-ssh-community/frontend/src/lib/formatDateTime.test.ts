import { describe, expect, it } from "vitest";
import { formatDateTime } from "./formatDateTime";

describe("formatDateTime", () => {
  const ts = Date.UTC(2024, 2, 5, 14, 7, 9);
  it("formats according to the given UI language, not the default locale", () => {
    const de = formatDateTime(ts, "de");
    const en = formatDateTime(ts, "en");
    expect(de).toBe(new Date(ts).toLocaleString("de"));
    expect(en).toBe(new Date(ts).toLocaleString("en"));
    expect(de).not.toBe(en);
  });
});
