// Spec 0055, Teil 4 ("Testbarkeit"): "eine Registrierung mit festem String
// ohne passenden Key bricht nicht."
import { describe, expect, it } from "vitest";
import { resolveSectionLabel } from "./resolveSectionLabel";
import { testI18n } from "./testI18n";

describe("resolveSectionLabel", () => {
  it("translates a label that matches a real i18next key", () => {
    expect(
      resolveSectionLabel("settings.categories.mcpServer", "mcp-server", testI18n, testI18n.t),
    ).toBe("MCP-Server");
  });

  it("displays a fixed string unchanged when it matches no translation key (backward compat)", () => {
    // Genau der Fall, den Spec 0055 Teil 4 explizit verlangt: eine
    // bestehende Registrierung mit einem festen Anzeigetext (z. B. eine
    // private/externe Sektion) darf nicht brechen.
    expect(resolveSectionLabel("Ganz normaler fester Text", "some-id", testI18n, testI18n.t)).toBe(
      "Ganz normaler fester Text",
    );
  });

  it("falls back to the id when label is undefined", () => {
    expect(resolveSectionLabel(undefined, "some-id", testI18n, testI18n.t)).toBe("some-id");
  });

  it("works against a minimal structural i18n/t pair (no real i18next instance required)", () => {
    const fakeI18n = { exists: (key: string) => key === "known.key" };
    const fakeT = (key: string) => `translated:${key}`;

    expect(resolveSectionLabel("known.key", "id", fakeI18n, fakeT)).toBe("translated:known.key");
    expect(resolveSectionLabel("unknown.key", "id", fakeI18n, fakeT)).toBe("unknown.key");
  });
});
