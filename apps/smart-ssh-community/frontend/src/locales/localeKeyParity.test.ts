import { describe, expect, it } from "vitest";
import { LANGUAGE_REGISTRY } from "../languages";
import allowlist from "./lengthAllowlist.json";
import en from "./en/common.json";
import identicalAllowlist from "./identicalAllowlist.json";
import { identicalProblems, keyParityProblems, lengthProblems, markerProblems } from "./parity";

// Spec 0072, C2 / Spec 0024: jede registrierte Sprache hat dieselben
// Schlüssel wie Englisch; Pluralformen folgen `Intl.PluralRules` der Sprache.
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

    it.runIf(["fr", "es", "it"].includes(code))("enthält alle Platzhalter und Tags des Englischen unverändert", () => {
      expect(markerProblems(resources, en)).toEqual([]);
    });

    // Gilt nur für die Sprachen aus Issue #331; de/pt-BR/… sind älter.
    it.runIf(["fr", "es", "it"].includes(code))("hat keinen Wert, der dem Englischen gleicht (Ausnahmen: identicalAllowlist.json)", () => {
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
