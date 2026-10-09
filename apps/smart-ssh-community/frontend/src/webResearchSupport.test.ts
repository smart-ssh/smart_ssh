import { describe, expect, it } from "vitest";
import { supportsWebResearch } from "./types";

describe("supportsWebResearch (Spec 0105 §1/§2)", () => {
  it("is on for Anthropic and the official OpenAI endpoint", () => {
    expect(supportsWebResearch("anthropic")).toBe(true);
    expect(supportsWebResearch("openai")).toBe(true);
    expect(supportsWebResearch("openai", null)).toBe(true);
    expect(supportsWebResearch("openai", "https://api.openai.com/v1/")).toBe(true);
  });

  it("is off for OpenAI with a custom base URL and for compatible endpoints", () => {
    expect(supportsWebResearch("openai", "https://proxy.example.com/v1")).toBe(false);
    expect(supportsWebResearch("generic_openai_compatible")).toBe(false);
    expect(supportsWebResearch("ollama")).toBe(false);
  });
});
