import { describe, expect, it } from "vitest";
import { supportsModelDiscovery } from "./types";

// Spec 0072, B-T3: `anthropic` unterstützt Modell-Discovery seit dieser
// Spec — die frühere Annahme (Spec 0025, Abschnitt 2), Anthropic habe "kein
// äquivalentes Endpoint-Verhalten", traf nicht zu (Spec 0072 §1). *Gegenbeweis:*
// vor dieser Änderung lieferte `supportsModelDiscovery("anthropic")` `false`
// — dieser Test schlug fehl, bis die Funktion erweitert wurde.
describe("supportsModelDiscovery (Spec 0072, B2/B3)", () => {
  it("unterstützt anthropic", () => {
    expect(supportsModelDiscovery("anthropic")).toBe(true);
  });

  it("unterstützt weiterhin die OpenAI-kompatible Familie und Ollama", () => {
    expect(supportsModelDiscovery("openai")).toBe(true);
    expect(supportsModelDiscovery("generic_openai_compatible")).toBe(true);
    expect(supportsModelDiscovery("ollama")).toBe(true);
  });
});
