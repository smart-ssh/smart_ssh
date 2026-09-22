import { describe, expect, it } from "vitest";
import { effectiveApiKey, OLLAMA_PLACEHOLDER_API_KEY } from "./ollama";

// Spec 0069, Teil B, Test 26. *Gegenbeweis (s. Bericht):* vor Teil B gab es
// `effectiveApiKey` nicht — jeder dieser Fälle hätte den rohen (ggf.
// leeren) `form.apiKey` durchgereicht.
describe("effectiveApiKey", () => {
  it("Ollama + leer -> Platzhalter", () => {
    expect(effectiveApiKey({ providerType: "ollama", apiKey: "" })).toBe(
      OLLAMA_PLACEHOLDER_API_KEY,
    );
  });

  it("Ollama + nur Leerraum -> Platzhalter", () => {
    expect(effectiveApiKey({ providerType: "ollama", apiKey: "   " })).toBe(
      OLLAMA_PLACEHOLDER_API_KEY,
    );
  });

  it("Ollama + Wert -> Wert unverändert", () => {
    expect(effectiveApiKey({ providerType: "ollama", apiKey: "sk-real" })).toBe("sk-real");
  });

  it("anderer Typ + leer -> leer (kein Platzhalter für Cloud-Provider)", () => {
    expect(effectiveApiKey({ providerType: "openai", apiKey: "" })).toBe("");
    expect(effectiveApiKey({ providerType: "anthropic", apiKey: "" })).toBe("");
    expect(effectiveApiKey({ providerType: "generic_openai_compatible", apiKey: "" })).toBe("");
  });

  it("anderer Typ + Wert -> Wert unverändert", () => {
    expect(effectiveApiKey({ providerType: "openai", apiKey: "sk-abc" })).toBe("sk-abc");
  });
});
