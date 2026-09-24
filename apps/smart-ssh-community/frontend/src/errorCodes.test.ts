import { describe, expect, it } from "vitest";
import { FIVE_MINUTE_PATH_ERROR_CODES, translateErrorCode } from "./errorCodes";
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

// Spec 0069, Teil A, Test 18: jeder Code des Fünf-Minuten-Pfads ist in
// KNOWN_ERROR_CODES (indirekt geprüft: `translateErrorCode` liefert für
// einen bekannten Code nie den Fallback zurück), hat einen nicht-leeren
// DE- und EN-Text, und DE unterscheidet sich von EN (kein vergessener
// Copy-Paste-Platzhalter). *Gegenbeweis:* vor Spec 0069 fehlten
// FIVE_MINUTE_PATH_ERROR_CODES und die neuen Codes komplett — dieser Test
// schlug fehl (Import-Fehler bzw. leere Liste).
describe("FIVE_MINUTE_PATH_ERROR_CODES", () => {
  const FALLBACK = "__FALLBACK_SENTINEL__";

  it.each(FIVE_MINUTE_PATH_ERROR_CODES)("%s ist bekannt und DE/EN unterscheiden sich", (code) => {
    const de = translateErrorCode(testI18n.getFixedT("de"), code, FALLBACK);
    const en = translateErrorCode(testI18n.getFixedT("en"), code, FALLBACK);

    expect(de).not.toBe(FALLBACK);
    expect(en).not.toBe(FALLBACK);
    expect(de.trim().length).toBeGreaterThan(0);
    expect(en.trim().length).toBeGreaterThan(0);
    expect(de).not.toBe(en);
  });

  it("enthält keine doppelten Codes", () => {
    const unique = new Set(FIVE_MINUTE_PATH_ERROR_CODES);
    expect(unique.size).toBe(FIVE_MINUTE_PATH_ERROR_CODES.length);
  });
});

// Spec 0077, T-6 (3.1.4/3.1.6): Ohne den Eintrag in `KNOWN_ERROR_CODES`
// zeigte das Regel-Formular den rohen Bibliothekstext der `regex`/
// `globset`-Crate ("... unclosed group") statt eines Satzes, der sagt, was
// zu tun ist.
describe("FILTER_RULE_PATTERN_INVALID (Spec 0077)", () => {
  const RAW = "regex parse error: unclosed group";

  it.each(["de", "en"] as const)(
    "übersetzt den Code (%s) statt den Rohcode oder den Bibliothekstext zu zeigen",
    (language) => {
      const text = translateErrorCode(
        testI18n.getFixedT(language),
        "FILTER_RULE_PATTERN_INVALID",
        RAW,
      );

      expect(text).not.toBe(RAW);
      expect(text).not.toBe("FILTER_RULE_PATTERN_INVALID");
      expect(text).not.toMatch(/^errors\./);
      expect(text.trim().length).toBeGreaterThan(0);
      // Muss das Muster als Ursache benennen, nicht nur "Fehler".
      expect(text).toMatch(language === "de" ? /muster/i : /pattern/i);
    },
  );

  it("DE und EN sind eigene Texte, nicht derselbe String", () => {
    const de = translateErrorCode(testI18n.getFixedT("de"), "FILTER_RULE_PATTERN_INVALID", RAW);
    const en = translateErrorCode(testI18n.getFixedT("en"), "FILTER_RULE_PATTERN_INVALID", RAW);

    expect(de).not.toBe(en);
  });
});
