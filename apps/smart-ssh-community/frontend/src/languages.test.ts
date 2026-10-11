import { describe, expect, it } from "vitest";
import {
  isSupportedLanguage,
  LANGUAGE_REGISTRY,
  languageForLocale,
  languagesForPicker,
  SUPPORTED_LANGUAGES,
  type LanguageEntry,
} from "./languages";

const ALL = ["de", "en", "pt-BR", "zh-Hans", "nb", "fr", "pl", "cs", "uk", "ru", "ja", "ko"];

describe("languageForLocale", () => {
  it.each([
    ["de-DE", "de"],
    ["en-US", "en"],
    ["pt-BR", "pt-BR"],
    ["pt-PT", "pt-BR"],
    ["pt", "pt-BR"],
    ["zh-CN", "zh-Hans"],
    ["zh-SG", "zh-Hans"],
    ["zh-Hans-CN", "zh-Hans"],
    ["zh-Hans-HK", "zh-Hans"],
    ["zh-Hans-TW", "zh-Hans"],
    ["zh", "zh-Hans"],
    ["zh-TW", "en"],
    ["zh-HK", "en"],
    ["zh-MO", "en"],
    ["zh-Hant-HK", "en"],
    ["zh_TW", "en"],
    ["nn-NO", "nb"],
    ["no", "nb"],
    ["nb-NO", "nb"],
    ["fr_FR", "fr"],
    ["pl-PL", "pl"],
    ["cs-CZ", "cs"],
    ["uk-UA", "uk"],
    ["ru-RU", "ru"],
    ["ru", "ru"],
    ["ja-JP", "ja"],
    ["ko-KR", "ko"],
    ["zh_CN", "zh-Hans"],
    ["xx-YY", "en"],
    ["", "en"],
  ])("%s -> %s", (raw, expected) => {
    expect(languageForLocale(raw, ALL)).toBe(expected);
  });

  it("falls back to en when the target language is not registered", () => {
    expect(languageForLocale("pt-BR", ["de", "en"])).toBe("en");
    expect(languageForLocale("nn-NO", ["de", "en"])).toBe("en");
    expect(languageForLocale("fr-FR", ["de", "en"])).toBe("en");
    expect(languageForLocale(null, ["de", "en"])).toBe("en");
  });
});

describe("registry", () => {
  it("lists registered codes and rejects unregistered stored values", () => {
    expect(SUPPORTED_LANGUAGES).toEqual(LANGUAGE_REGISTRY.map((l) => l.code));
    expect(isSupportedLanguage("de")).toBe(true);
    expect(isSupportedLanguage("xx")).toBe(false);
    expect(isSupportedLanguage(null)).toBe(false);
  });

  it("sorts the picker by English name and carries the endonym", () => {
    const fake: LanguageEntry = { code: "zz", endonym: "Zzz", englishName: "Aardvarkish", resources: {} };
    const sorted = languagesForPicker([...LANGUAGE_REGISTRY, fake]);
    expect(sorted.map((l) => l.englishName)).toEqual(["Aardvarkish", "Chinese (Simplified)", "Czech", "Dutch", "English", "French", "German", "Italian", "Japanese", "Korean", "Polish", "Portuguese (Brazil)", "Russian", "Spanish", "Ukrainian"]);
    expect(languagesForPicker().map((l) => l.endonym)).toEqual(["简体中文", "Čeština", "Nederlands", "English", "Français", "Deutsch", "Italiano", "日本語", "한국어", "Polski", "Português (Brasil)", "Русский", "Español", "Українська"]);
  });
});
