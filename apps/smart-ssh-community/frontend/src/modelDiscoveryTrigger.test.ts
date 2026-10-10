import { describe, expect, it } from "vitest";
import { discoveryInputsOf, shouldAutoDiscover } from "./modelDiscoveryTrigger";

describe("shouldAutoDiscover (Spec 0025, Abschnitt 2, issue #326)", () => {
  const openai = { providerType: "openai" as const, baseUrl: null, apiKey: "sk-1" };

  it("triggers for a supported type with a key and no previous discovery", () => {
    expect(shouldAutoDiscover(openai, null)).toBe(true);
  });

  it("does not trigger with an empty or whitespace-only key", () => {
    expect(shouldAutoDiscover({ ...openai, apiKey: "" }, null)).toBe(false);
    expect(shouldAutoDiscover({ ...openai, apiKey: "  " }, null)).toBe(false);
  });

  it("does not trigger for Ollama", () => {
    expect(
      shouldAutoDiscover(
        { providerType: "ollama", baseUrl: "http://127.0.0.1:11434/v1", apiKey: "x" },
        null,
      ),
    ).toBe(false);
  });

  it("requires a base URL where the type needs one", () => {
    const generic = { providerType: "generic_openai_compatible" as const, apiKey: "k" };
    expect(shouldAutoDiscover({ ...generic, baseUrl: null }, null)).toBe(false);
    expect(shouldAutoDiscover({ ...generic, baseUrl: " " }, null)).toBe(false);
    expect(shouldAutoDiscover({ ...generic, baseUrl: "https://x/v1" }, null)).toBe(true);
  });

  it("does not trigger again for unchanged inputs, but does for any changed input", () => {
    const last = discoveryInputsOf(openai);
    expect(shouldAutoDiscover(openai, last)).toBe(false);
    expect(shouldAutoDiscover({ ...openai, apiKey: "sk-2" }, last)).toBe(true);
    expect(shouldAutoDiscover({ ...openai, providerType: "anthropic" }, last)).toBe(true);
    expect(shouldAutoDiscover({ ...openai, baseUrl: "https://other/v1" }, last)).toBe(true);
  });
});
