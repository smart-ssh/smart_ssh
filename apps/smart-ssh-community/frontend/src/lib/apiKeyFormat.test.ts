import { describe, expect, it } from "vitest";
import { apiKeyFormatWarning } from "./apiKeyFormat";

describe("apiKeyFormatWarning (Spec 0050, Teil 2)", () => {
  it("warns for an Anthropic key without the sk-ant- prefix", () => {
    expect(apiKeyFormatWarning("anthropic", null, "wrong-prefix-123")).toEqual({
      expectedPrefix: "sk-ant-",
    });
  });

  it("does not warn for a correctly prefixed Anthropic key", () => {
    expect(apiKeyFormatWarning("anthropic", null, "sk-ant-abc123")).toBeNull();
  });

  it("does not warn for a correctly prefixed OpenAI key", () => {
    expect(apiKeyFormatWarning("openai", null, "sk-abc123")).toBeNull();
  });

  it("does not warn for an OpenAI project key (sk-proj- is itself an sk- prefix)", () => {
    expect(apiKeyFormatWarning("openai", null, "sk-proj-abc123")).toBeNull();
  });

  it("warns for an OpenAI key without the sk- prefix", () => {
    expect(apiKeyFormatWarning("openai", null, "not-an-openai-key")).toEqual({
      expectedPrefix: "sk-",
    });
  });

  it("warns for an OpenRouter key (generic provider pointed at openrouter.ai) without sk-or-", () => {
    expect(
      apiKeyFormatWarning("generic_openai_compatible", "https://openrouter.ai/api/v1", "sk-abc"),
    ).toEqual({ expectedPrefix: "sk-or-" });
  });

  it("does not warn for a correctly prefixed OpenRouter key", () => {
    expect(
      apiKeyFormatWarning(
        "generic_openai_compatible",
        "https://openrouter.ai/api/v1",
        "sk-or-abc123",
      ),
    ).toBeNull();
  });

  it("never warns for a generic OpenAI-compatible provider with an unrelated base URL", () => {
    expect(
      apiKeyFormatWarning("generic_openai_compatible", "https://my-self-hosted.example/v1", "anything"),
    ).toBeNull();
  });

  it("never warns for a generic OpenAI-compatible provider with no base URL yet", () => {
    expect(apiKeyFormatWarning("generic_openai_compatible", null, "anything")).toBeNull();
  });

  it("never warns for ollama regardless of key content", () => {
    expect(apiKeyFormatWarning("ollama", null, "anything-at-all")).toBeNull();
  });

  it("never warns for an empty key (that's a required-field concern, not a format concern)", () => {
    expect(apiKeyFormatWarning("anthropic", null, "")).toBeNull();
    expect(apiKeyFormatWarning("anthropic", null, "   ")).toBeNull();
  });
});
