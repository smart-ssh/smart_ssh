import { describe, expect, it } from "vitest";
import { LANGUAGE_REGISTRY } from "../languages";
import allowlist from "./lengthAllowlist.json";
import en from "./en/common.json";
import identicalAllowlist from "./identicalAllowlist.json";
import { identicalProblems, keyParityProblems, leafValues, lengthProblems, markerProblems } from "./parity";

// Spec 0072, C2 / Spec 0024: jede registrierte Sprache hat dieselben
// Schlüssel wie Englisch; Pluralformen folgen `Intl.PluralRules` der Sprache.
const NEW_LANGUAGES = ["fr", "es", "it", "pl", "cs", "uk", "ru", "ja", "zh-Hans", "ko", "sv", "da", "nb", "fi", "tr", "el", "hu", "ro"];

// Neue Sprachen kommen allein über die Registry (`languages.ts`) dazu.
describe.each(LANGUAGE_REGISTRY.filter((l) => l.code !== "en"))(
  "Übersetzung $code gegen Englisch",
  ({ code, resources }) => {
    it("hat genau die Schlüssel des Englischen (Plurale nach Sprache)", () => {
      expect(keyParityProblems(code, resources, en)).toEqual([]);
    });

    it("hält die Längengrenze 2 × Englisch + 10 ein (Ausnahmen: lengthAllowlist.json)", () => {
      const allowed = (allowlist as Record<string, Record<string, string>>)[code] ?? {};
      expect(lengthProblems(resources, en, allowed)).toEqual([]);
    });

    it.runIf(NEW_LANGUAGES.includes(code))("enthält alle Platzhalter und Tags des Englischen unverändert", () => {
      expect(markerProblems(resources, en)).toEqual([]);
    });

    // Gilt nur für die Sprachen aus Issue #331/#334/#335; de/pt-BR/… sind älter.
    it.runIf(NEW_LANGUAGES.includes(code))("hat keinen Wert, der dem Englischen gleicht (Ausnahmen: identicalAllowlist.json)", () => {
      const allowed = (identicalAllowlist as Record<string, Record<string, string>>)[code] ?? {};
      expect(identicalProblems(resources, en, allowed)).toEqual([]);
    });
  },
);

describe("Allowlist der Gleichheitsprüfung", () => {
  it("nennt nur registrierte Sprachen und gibt je Schlüssel einen Grund an", () => {
    const codes = new Set(LANGUAGE_REGISTRY.map((l) => l.code));
    for (const [lang, keys] of Object.entries(identicalAllowlist as Record<string, Record<string, string>>)) {
      expect(codes.has(lang), lang).toBe(true);
      for (const [key, reason] of Object.entries(keys)) {
        expect(reason.trim().length, `${lang}:${key}`).toBeGreaterThan(0);
      }
    }
  });
});

describe("markerProblems / identicalProblems", () => {
  it("erkennt veränderte Platzhalter und Tags", () => {
    expect(markerProblems({ a: "{{n}} x" }, { a: "{{count}} x" })).toHaveLength(1);
    expect(markerProblems({ a: "<0>x</0>" }, { a: "<0>x</0>" })).toEqual([]);
    expect(markerProblems({ a: "x" }, { a: "<0>x</0>" })).toHaveLength(1);
  });
  it("erkennt gleiche Werte, ignoriert reine Platzhalter", () => {
    expect(identicalProblems({ a: "Open", b: "{{n}}" }, { a: "Open", b: "{{n}}" })).toEqual(["a: identical to English"]);
    expect(identicalProblems({ a: "Open" }, { a: "Open" }, { a: "same word" })).toEqual([]);
  });
});

describe("Allowlist der Längenprüfung", () => {
  it("nennt nur registrierte Sprachen und gibt je Schlüssel einen Grund an", () => {
    const codes = new Set(LANGUAGE_REGISTRY.map((l) => l.code));
    for (const [lang, keys] of Object.entries(allowlist as Record<string, Record<string, string>>)) {
      expect(codes.has(lang), lang).toBe(true);
      for (const [key, reason] of Object.entries(keys)) {
        expect(reason.trim().length, `${lang}:${key}`).toBeGreaterThan(0);
      }
    }
  });
});

describe("keyParityProblems (Pluralformen)", () => {
  const english = { n: { files_one: "{{count}} file", files_other: "{{count}} files" }, a: "x" };

  it("verlangt für Polnisch auch _few und _many", () => {
    const pl = { n: { files_one: "a", files_other: "b" }, a: "x" };
    expect(keyParityProblems("pl", pl, english)).toEqual(["missing: n.files_few", "missing: n.files_many"]);
  });

  it("besteht, wenn alle Polnisch-Formen vorhanden sind", () => {
    const pl = { n: { files_one: "a", files_few: "c", files_many: "d", files_other: "b" }, a: "x" };
    expect(keyParityProblems("pl", pl, english)).toEqual([]);
  });

  it("meldet überzählige Pluralformen und fehlende normale Schlüssel", () => {
    const de = { n: { files_one: "a", files_few: "c", files_other: "b" } };
    expect(keyParityProblems("de", de, english)).toEqual(["missing: a", "unexpected: n.files_few"]);
  });
});

describe("lengthProblems", () => {
  it("meldet zu lange Werte, außer sie stehen in der Ausnahmeliste", () => {
    const english = { k: "abc" };
    const long = { k: "x".repeat(17) };
    expect(lengthProblems(long, english)).toEqual(["k: 17 > 16"]);
    expect(lengthProblems(long, english, { k: "reason" })).toEqual([]);
    expect(lengthProblems({ k: "x".repeat(16) }, english)).toEqual([]);
  });
});

// Issue #332: Platzhalter und Tags des Englischen bleiben unverändert; Werte
// sind nicht mit Englisch identisch (außer Platzhalter-only, Produkt-/Fachbegriffe
// und Wörter, die in der Sprache gleich lauten).
// Wörter, die in der Sprache wie im Englischen lauten (Lehnwörter oder
// Fachbegriffe, im Glossar nicht übersetzt). Gilt für Werte, die nach Abzug von
// Platzhaltern und Satzzeichen nur aus diesen Wörtern bestehen. URLs, Pfade
// und Platzhalter-only-Werte sind immer erlaubt.
const SAME_WORDS: Record<string, readonly string[]> = {
  "pt-BR": ["ok", "mcp", "logs", "host", "tags", "tag", "enter", "global", "terminal", "token", "local", "mac"],
  id: ["ok", "mcp", "server", "host", "token", "port", "proxy", "mac", "terminal", "tag", "tags", "log", "logs", "model", "ai", "api", "url", "default", "global", "online", "offline", "email", "pin"],
  vi: ["ok", "mcp", "server", "host", "token", "proxy", "mac", "terminal", "tag", "tags", "log", "logs", "ai", "api", "url", "email", "pin"],
  hi: ["ok", "mcp"],
  nl: [
    "ok", "mcp", "server", "servers", "database", "host", "type", "model", "token", "details",
    "later", "terminal", "hop", "cipher", "mac", "extra", "headers", "header", "openai",
    "anthropic", "ollama", "ai",
  ],
};
// Produktnamen sind in allen Sprachen gleich (Glossar: bleiben unübersetzt).
const PRODUCT_WORDS = ["openai", "anthropic", "ollama", "mcp", "ok"];
function isAllowedSame(code: string, value: string): boolean {
  if (/^(https?:|~\/|\/)/.test(value.trim())) return true;
  const words = stripped(value)
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
  const ok = new Set([...(SAME_WORDS[code] ?? []), ...PRODUCT_WORDS]);
  return words.every((w) => ok.has(w));
}
const tokens = (s: string) => (s.match(/\{\{[^}]*\}\}|<\/?[a-zA-Z0-9]+\s*\/?>/g) ?? []).sort();
const stripped = (s: string) => s.replace(/\{\{[^}]*\}\}|<\/?[a-zA-Z0-9]+\s*\/?>/g, "").trim();

describe.each(LANGUAGE_REGISTRY.filter((l) => ["pt-BR", "nl", "id", "vi", "hi"].includes(l.code)))(
  "Platzhalter und Eigenständigkeit $code",
  ({ code, resources }) => {
    const enValues = leafValues(en);
    const values = leafValues(resources);

    it("übernimmt alle {{…}}-Platzhalter und Tags des Englischen unverändert", () => {
      const problems: string[] = [];
      for (const [key, value] of values) {
        const ref = enValues.get(key) ?? enValues.get(key.replace(/_[a-z]+$/, "_other"));
        if (ref === undefined) continue;
        if (JSON.stringify(tokens(ref)) !== JSON.stringify(tokens(value))) problems.push(key);
      }
      expect(problems).toEqual([]);
    });

    it("hat keinen Wert identisch zum Englischen (Ausnahmen: SAME_WORDS)", () => {
      const problems: string[] = [];
      for (const [key, value] of values) {
        const ref = enValues.get(key);
        if (ref === undefined || ref !== value) continue;
        if (stripped(value).length === 0 || isAllowedSame(code, value)) continue;
        problems.push(`${key}: ${value}`);
      }
      expect(problems).toEqual([]);
    });
  },
);
