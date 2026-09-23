import { describe, expect, it } from "vitest";
import de from "./de/common.json";
import en from "./en/common.json";

/** Sammelt jeden Blatt-Schlüsselpfad eines (verschachtelten) Übersetzungs-
 * Objekts, z. B. `{ sidebar: { addGroup: "..." } }` -> `["sidebar.addGroup"]`. */
function leafKeyPaths(value: unknown, prefix = ""): string[] {
  if (value !== null && typeof value === "object" && !Array.isArray(value)) {
    return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) =>
      leafKeyPaths(child, prefix ? `${prefix}.${key}` : key),
    );
  }
  return [prefix];
}

// Spec 0072, C2: "Beide Sprachdateien bekommen den Schlüssel; ein Test oder
// Lint, der fehlende Gegenstücke findet, bleibt grün." Es gab bislang keinen
// Test, der DE/EN-Schlüsselparität für die *gesamte* `common.json` prüft
// (nur `errorCodes.test.ts`s `FIVE_MINUTE_PATH_ERROR_CODES` deckte einen
// Ausschnitt ab) — dieser Test schließt die Lücke. *Gegenbeweis:* mit nur
// einer der beiden `management.emptyState`-Zeilen aus Teil 3 auskommentiert
// schlägt dieser Test fehl (verifiziert, danach wiederhergestellt).
describe("DE/EN-Übersetzungsdateien haben identische Schlüsselmengen", () => {
  it("kein Schlüssel fehlt in einer der beiden Sprachen", () => {
    const deKeys = new Set(leafKeyPaths(de));
    const enKeys = new Set(leafKeyPaths(en));

    const onlyInDe = [...deKeys].filter((key) => !enKeys.has(key)).sort();
    const onlyInEn = [...enKeys].filter((key) => !deKeys.has(key)).sort();

    expect(onlyInDe, `nur in DE vorhanden: ${onlyInDe.join(", ")}`).toEqual([]);
    expect(onlyInEn, `nur in EN vorhanden: ${onlyInEn.join(", ")}`).toEqual([]);
  });
});
