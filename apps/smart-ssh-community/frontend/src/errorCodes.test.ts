import { describe, expect, it } from "vitest";
import { translateErrorCode } from "./errorCodes";
import { testI18n } from "./testI18n";

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

  // Spec 0051, Teil 3: `AI_RATE_LIMITED` muss im UI ein eigener, vom
  // `AI_PROVIDER_UNAVAILABLE`-Sammeltopf unterscheidbarer Fall sein UND
  // verständlich/handlungsanleitend formuliert sein ("kurz warten und
  // erneut senden"), statt der generischen "Provider-Konfiguration
  // prüfen"-Meldung. Nutzt `testI18n` (echte `de`/`en`-Ressourcen, s.
  // dortiger Kommentar), nicht `fakeT` — sonst würde dieser Test nur die
  // Weiterleitungslogik prüfen, nicht den tatsächlichen Text.
  it("übersetzt AI_RATE_LIMITED (DE) in einen eigenen, handlungsanleitenden Text", () => {
    const text = translateErrorCode(testI18n.getFixedT("de"), "AI_RATE_LIMITED", "fallback");

    expect(text).not.toBe("fallback");
    expect(text).not.toBe(
      translateErrorCode(testI18n.getFixedT("de"), "AI_PROVIDER_UNAVAILABLE", "fallback"),
    );
    expect(text).toMatch(/warten/i);
    expect(text).toMatch(/erneut/i);
  });

  it("übersetzt AI_RATE_LIMITED (EN) in einen eigenen, handlungsanleitenden Text", () => {
    const text = translateErrorCode(testI18n.getFixedT("en"), "AI_RATE_LIMITED", "fallback");

    expect(text).not.toBe("fallback");
    expect(text).not.toBe(
      translateErrorCode(testI18n.getFixedT("en"), "AI_PROVIDER_UNAVAILABLE", "fallback"),
    );
    expect(text).toMatch(/wait/i);
    expect(text).toMatch(/again/i);
  });

  // Spec 0071, A13/X2: Der Backend-Fehler kommt als
  // `{ code: "KEYCHAIN_UNAVAILABLE" }` — das Frontend muss den eigenen,
  // übersetzten Text zeigen, nicht den `message`-Fallback. Ohne den Eintrag
  // in `KNOWN_ERROR_CODES` stünde hier der rohe englische
  // `keyring`-Bibliothekstext, also genau der Fehler aus BL-0031.
  it.each(["de", "en"] as const)(
    "übersetzt KEYCHAIN_UNAVAILABLE (%s) statt den rohen Bibliothekstext zu zeigen",
    (language) => {
      const raw = "Credential-Backend-Fehler: No default store has been set";
      const text = translateErrorCode(testI18n.getFixedT(language), "KEYCHAIN_UNAVAILABLE", raw);

      expect(text).not.toBe(raw);
      expect(text).not.toMatch(/no default store/i);
      // Muss sagen, was blockiert ist (A5 b), nicht nur "Fehler".
      expect(text).toMatch(/API/i);
      expect(text).toMatch(/passphrase/i);
    },
  );
});
