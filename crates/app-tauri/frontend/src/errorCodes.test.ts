import { describe, expect, it } from "vitest";
import { translateErrorCode } from "./errorCodes";

/** Testdouble für `useTranslation()`s `t` — löst bekannte Keys auf einen
 * erkennbaren String auf, damit sich Treffer/Fallback klar unterscheiden. */
const fakeT = (key: string) => `translated:${key}`;

describe("translateErrorCode", () => {
  it("übersetzt einen bekannten Code über den errors-Namespace", () => {
    expect(translateErrorCode(fakeT, "SSH_AUTH_FAILED", "Auth fehlgeschlagen")).toBe(
      "translated:errors.SSH_AUTH_FAILED",
    );
  });

  // Spec 0024, Abschnitt 5: unbekannter Code -> Display-Text als Fallback,
  // nie eine leere Anzeige, kein Absturz.
  it("fällt bei unbekanntem Code auf den Display-Text zurück", () => {
    expect(translateErrorCode(fakeT, "SOME_FUTURE_CODE_NOT_YET_MAPPED", "Ursprünglicher Text")).toBe(
      "Ursprünglicher Text",
    );
  });

  it("fällt bei fehlendem Code (null/undefined) auf den Display-Text zurück", () => {
    expect(translateErrorCode(fakeT, null, "Ursprünglicher Text")).toBe("Ursprünglicher Text");
    expect(translateErrorCode(fakeT, undefined, "Ursprünglicher Text")).toBe("Ursprünglicher Text");
  });

  it("fällt bei leerem Code-String auf den Display-Text zurück, kein Absturz", () => {
    expect(translateErrorCode(fakeT, "", "Ursprünglicher Text")).toBe("Ursprünglicher Text");
  });
});
