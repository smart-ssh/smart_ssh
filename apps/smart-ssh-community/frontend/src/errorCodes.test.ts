import { describe, expect, it } from "vitest";
import {
  FIVE_MINUTE_PATH_ERROR_CODES,
  MASTER_PASSWORD_ERROR_CODES,
  translateErrorCode,
} from "./errorCodes";
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

  // Issue #96: Verbindungsverlust einer laufenden Sitzung bekommt einen
  // eigenen Text, nicht den des Verbindungsaufbaus.
  it.each(["de", "en"] as const)(
    "übersetzt SSH_SESSION_CLOSED (%s) mit eigenem Text ohne Aufbau-Bezug",
    (language) => {
      const t = testI18n.getFixedT(language);
      const text = translateErrorCode(t, "SSH_SESSION_CLOSED", "fallback");

      expect(text).not.toBe("fallback");
      expect(text).not.toBe("errors.SSH_SESSION_CLOSED");
      expect(text).not.toBe(translateErrorCode(t, "SSH_CONNECTION_CLOSED", "fallback"));
      expect(text).not.toMatch(/setup|aufbau/i);
      expect(text).toMatch(language === "de" ? /erneut verbinden/i : /reconnect/i);
    },
  );

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

  // Spec 0098, T8 (A6): Derselbe Weg für den neuen Code. Der Fallback wäre
  // hier der `Display`-Text aus `core` — der ist zwar seit Spec 0098 frei von
  // der Nutzlast der Bibliothek, aber deutsch und technisch („Zugriff auf den
  // Schlüsselbund fehlgeschlagen (Passphrase)"). Ohne den Eintrag in
  // `KNOWN_ERROR_CODES` sähe ein Nutzer mit englischer UI genau den.
  it.each(["de", "en"] as const)(
    "übersetzt KEYCHAIN_ACCESS_FAILED (%s) statt den Backend-Text zu zeigen",
    (language) => {
      const raw = "Zugriff auf den Schlüsselbund fehlgeschlagen (Passphrase)";
      const text = translateErrorCode(testI18n.getFixedT(language), "KEYCHAIN_ACCESS_FAILED", raw);

      expect(text).not.toBe(raw);
      // A6: sagt, was zu tun ist, und nennt die wahrscheinlichen Ursachen.
      expect(text).toMatch(language === "de" ? /erneut/i : /again/i);
      expect(text).toMatch(language === "de" ? /gesperrt|abgelehnt/i : /locked|denied/i);
      // A6: **nicht** „nicht verfügbar" — der Schlüsselbund ist da, dieser
      // eine Zugriff ist gescheitert (A3). Und keine Paket- oder
      // Installationshinweise.
      expect(text).not.toMatch(language === "de" ? /nicht verfügbar/i : /unavailable/i);
      expect(text).not.toMatch(/installier|install|paket|package|apt|brew/i);
    },
  );

  // Spec 0098, T8/A6: Die beiden Schlüsselbund-Codes sind unterschiedliche
  // Aussagen und dürfen nicht denselben Text bekommen — sonst wäre der
  // ganze Unterschied zwischen A1 und A2 für den Nutzer unsichtbar.
  it.each(["de", "en"] as const)(
    "unterscheidet KEYCHAIN_ACCESS_FAILED von KEYCHAIN_UNAVAILABLE (%s)",
    (language) => {
      const t = testI18n.getFixedT(language);

      expect(translateErrorCode(t, "KEYCHAIN_ACCESS_FAILED", "fallback")).not.toBe(
        translateErrorCode(t, "KEYCHAIN_UNAVAILABLE", "fallback"),
      );
    },
  );

  // Spec 0101, A9/A9.1/A20 (Rest): Secrets liegen seit Etappe 2 in der
  // verschlüsselten Datenbank, nicht mehr im Schlüsselbund. Ohne den
  // Eintrag in `KNOWN_ERROR_CODES` wäre `SECRET_STORE_FAILED` unbekannt und
  // der `message`-Fallback (ein technischer Backend-Text) erschiene statt
  // eines übersetzten Satzes. *Gegenbeweis:* Entfernt man den Eintrag aus
  // `KNOWN_ERROR_CODES`, liefert `translateErrorCode` den `raw`-Fallback,
  // und dieser Test scheitert an der ersten Zusicherung.
  it.each(["de", "en"] as const)(
    "übersetzt SECRET_STORE_FAILED (%s) statt den rohen Backend-Text zu zeigen",
    (language) => {
      const raw = "credential store backend failure: disk I/O error";
      const text = translateErrorCode(testI18n.getFixedT(language), "SECRET_STORE_FAILED", raw);

      expect(text).not.toBe(raw);
      // Muss sagen, dass es um ein Geheimnis in der Datenbank geht — nicht
      // um den Schlüsselbund (der trägt seit A9 nur noch den Wurzelschlüssel).
      expect(text).toMatch(language === "de" ? /Datenbank/i : /database/i);
      expect(text).not.toMatch(language === "de" ? /Schlüsselbund/i : /keychain/i);
    },
  );

  // Derselbe Grund wie bei den beiden Schlüsselbund-Codes oben: Ein
  // gescheiterter Secret-Zugriff und ein gescheiterter Schlüsselbund-Zugriff
  // sind unterschiedliche Ursachen mit unterschiedlichem nächsten Schritt.
  it.each(["de", "en"] as const)("unterscheidet SECRET_STORE_FAILED von KEYCHAIN_ACCESS_FAILED (%s)", (language) => {
    const t = testI18n.getFixedT(language);

    expect(translateErrorCode(t, "SECRET_STORE_FAILED", "fallback")).not.toBe(
      translateErrorCode(t, "KEYCHAIN_ACCESS_FAILED", "fallback"),
    );
  });
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

// Spec 0101, A20/T14 („die Codes der Etappe 3 übersetzt"): jeder Code des
// Master-Passworts ist bekannt und hat einen nicht-leeren, eigenen DE- und
// EN-Text. *Gegenbeweis:* Ein Code, der in `KNOWN_ERROR_CODES` steht und in
// einer Locale-Datei fehlt, liefert `errors.<CODE>` als rohen Schlüssel —
// dieser Test wird dann rot (geprüft, indem ein Schlüssel entfernt wurde).
describe("MASTER_PASSWORD_ERROR_CODES", () => {
  const FALLBACK = "__FALLBACK_SENTINEL__";

  it.each(MASTER_PASSWORD_ERROR_CODES)("%s ist bekannt und DE/EN unterscheiden sich", (code) => {
    const de = translateErrorCode(testI18n.getFixedT("de"), code, FALLBACK);
    const en = translateErrorCode(testI18n.getFixedT("en"), code, FALLBACK);

    expect(de).not.toBe(FALLBACK);
    expect(en).not.toBe(FALLBACK);
    // Ein fehlender Schlüssel käme als `errors.<CODE>` durch — das ist
    // kein Text, den ein Nutzer lesen soll.
    expect(de).not.toBe(`errors.${code}`);
    expect(en).not.toBe(`errors.${code}`);
    expect(de).not.toBe(en);
  });

  it("MASTER_PASSWORD_FILE_ABSENT behauptet keine unlesbare Datei (review-09, Runde 2 Teil A, Fund 2 „Rest“)", () => {
    // Vorher lief „keine Datei an diesem Ort“ unter demselben Code wie „Datei
    // liegt da, ist aber gerade nicht lesbar“ (MASTER_PASSWORD_FILE_FAILED)
    // — der gemeinsame Text behauptete dann eine unlesbare Datei, wo keine
    // existiert. *Gegenbeweis:* Setzt man hier denselben Text wie bei
    // MASTER_PASSWORD_FILE_FAILED ein, scheitert dieser Test.
    const de = translateErrorCode(testI18n.getFixedT("de"), "MASTER_PASSWORD_FILE_ABSENT", FALLBACK);
    const en = translateErrorCode(testI18n.getFixedT("en"), "MASTER_PASSWORD_FILE_ABSENT", FALLBACK);
    expect(de).not.toMatch(/nicht (lesbar|zu lesen)/);
    expect(en).not.toMatch(/could not be read/i);
    expect(de).not.toBe(
      translateErrorCode(testI18n.getFixedT("de"), "MASTER_PASSWORD_FILE_FAILED", FALLBACK),
    );
    expect(en).not.toBe(
      translateErrorCode(testI18n.getFixedT("en"), "MASTER_PASSWORD_FILE_FAILED", FALLBACK),
    );
  });

  it("behauptet in keinem Text eine Schlüsseldatei, wo es keine geben muss", () => {
    // Klarstellung 9, Punkt 5 (Review-Fund Runde 1 zu Commit 11): Diese
    // beiden Fälle treten im **Schlüsselbund**-Modus auf, in dem es keine
    // Verpackungsdatei gibt. Vorher fielen sie unter
    // `MASTER_PASSWORD_FILE_FAILED`, und der Nutzer las „Die Schlüsseldatei
    // neben deiner Datenbank ließ sich nicht lesen".
    for (const code of ["KEYCHAIN_KEY_MISMATCH", "MASTER_PASSWORD_MODE_MISMATCH"]) {
      expect(translateErrorCode(testI18n.getFixedT("de"), code, FALLBACK)).not.toMatch(
        /Schlüsseldatei/,
      );
      expect(translateErrorCode(testI18n.getFixedT("en"), code, FALLBACK)).not.toMatch(
        /key file/i,
      );
    }
  });
});

// Spec 0077, T-6 (3.1.4/3.1.6): Ohne den Eintrag in `KNOWN_ERROR_CODES`
// zeigte das Regel-Formular den rohen Bibliothekstext der `regex`/
// `globset`-Crate ("... unclosed group") statt eines Satzes, der sagt, was
// zu tun ist.
// Spec 0087, T12 (A1.5): der DE- und EN-Text von AI_CONTEXT_TOO_LARGE muss
// das jeweilige Label von aiProvider.maxTokensOverrideLabel enthalten —
// gelesen aus den echten Locale-Ressourcen (testI18n), kein hier
// kopiertes/erwartetes Textstück, damit ein künftig geändertes Label nicht
// unbemerkt aus dem Fehlertext herausfällt.
describe("AI_CONTEXT_TOO_LARGE (Spec 0087)", () => {
  it.each(["de", "en"] as const)(
    "(%s) nennt das Label der Max.-Antwortlänge-Einstellung",
    (language) => {
      const t = testI18n.getFixedT(language);
      const label = t("aiProvider.maxTokensOverrideLabel");
      const text = translateErrorCode(t, "AI_CONTEXT_TOO_LARGE", "fallback");

      expect(text).not.toBe("fallback");
      expect(text).toContain(label);
    },
  );
});

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

// Spec 0092, A2.1: der Text zu
// FILTER_RED_RISK_REQUIRES_CONFIRM muss sowohl den regulären Fall ("rot
// eingestuft") als auch den Überlängen-Fail-safe ("nicht einschätzbar", s.
// `action_exec.rs::red_risk_confirm_reason`) abdecken — der Dialog zeigt den
// FESTEN übersetzten Text zum Code, nicht den variablen `reason` aus dem
// Backend (Spec 0092, §4 "Anzeige des Grunds"), es gibt also nur diesen
// einen Text für beide Fälle.
describe("FILTER_RED_RISK_REQUIRES_CONFIRM (Spec 0092)", () => {
  it("(de) nennt sowohl 'rot' als auch den Überlängen-/Unsicherheitsfall", () => {
    const text = translateErrorCode(
      testI18n.getFixedT("de"),
      "FILTER_RED_RISK_REQUIRES_CONFIRM",
      "fallback",
    );
    expect(text).not.toBe("fallback");
    expect(text).toMatch(/rot/i);
    expect(text).toMatch(/nicht sicher einschätzbar/i);
    expect(text).toMatch(/bestätigung/i);
  });

  it("(en) covers both the red case and the cannot-be-assessed case", () => {
    const text = translateErrorCode(
      testI18n.getFixedT("en"),
      "FILTER_RED_RISK_REQUIRES_CONFIRM",
      "fallback",
    );
    expect(text).not.toBe("fallback");
    expect(text).toMatch(/red/i);
    expect(text).toMatch(/could not be assessed/i);
    expect(text).toMatch(/confirmation/i);
  });

  it("DE und EN sind eigene Texte", () => {
    const de = translateErrorCode(
      testI18n.getFixedT("de"),
      "FILTER_RED_RISK_REQUIRES_CONFIRM",
      "fallback",
    );
    const en = translateErrorCode(
      testI18n.getFixedT("en"),
      "FILTER_RED_RISK_REQUIRES_CONFIRM",
      "fallback",
    );
    expect(de).not.toBe(en);
  });
});

// Issue #109: Ein Kommando, das für die Secret-Pfad- bzw. sftp-server-Prüfung
// zu lang oder zu tief verschachtelt ist, bekommt einen eigenen Code — der
// Dialog darf dann nicht behaupten, es lese eine Datei mit Zugangsdaten.
describe("FILTER_COMMAND_NOT_ASSESSABLE_REQUIRES_CONFIRM (Issue #109)", () => {
  it("(de) zeigt den festgelegten Text", () => {
    expect(
      translateErrorCode(
        testI18n.getFixedT("de"),
        "FILTER_COMMAND_NOT_ASSESSABLE_REQUIRES_CONFIRM",
        "fallback",
      ),
    ).toBe(
      "Kommando ist zu lang oder zu tief verschachtelt für eine Prüfung – erfordert immer Bestätigung",
    );
  });

  it("(en) shows the specified text", () => {
    expect(
      translateErrorCode(
        testI18n.getFixedT("en"),
        "FILTER_COMMAND_NOT_ASSESSABLE_REQUIRES_CONFIRM",
        "fallback",
      ),
    ).toBe("Command is too long or too deeply nested to be checked – always requires confirmation");
  });
});
