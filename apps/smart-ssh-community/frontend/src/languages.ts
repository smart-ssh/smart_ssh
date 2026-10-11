import de from "./locales/de/common.json";
import en from "./locales/en/common.json";
import es from "./locales/es/common.json";
import fr from "./locales/fr/common.json";
import it from "./locales/it/common.json";

// Spec 0024, Abschnitt 3/4: die einzige Quelle der Wahrheit für die
// unterstützten UI-Sprachen. Eine neue Sprache = eine Locale-Datei + ein
// Eintrag hier. Picker, `i18n.ts`, `testI18n.ts` und die Übersetzungstests
// leiten sich aus dieser Liste ab. Bewusst statische Imports (kein
// Nachladen aus dem Netz). Die Rust-Wächtertest `test_every_supported_ui_
// language_has_a_keyword_list` liest die `code: "…"`-Zeilen dieser Datei —
// das Format "eine Zeile `code: "xx",` je Eintrag" nicht ändern.

export interface LanguageEntry {
  /** BCP-47-Code, der auch als i18next-Sprache und im Store dient. */
  code: string;
  /** Eigenname der Sprache, unabhängig von der aktuellen UI-Sprache. */
  endonym: string;
  /** Englischer Name — nur für die Sortierung im Picker. */
  englishName: string;
  resources: Record<string, unknown>;
}

export const LANGUAGE_REGISTRY: readonly LanguageEntry[] = [
  {
    code: "de",
    endonym: "Deutsch",
    englishName: "German",
    resources: de,
  },
  {
    code: "en",
    endonym: "English",
    englishName: "English",
    resources: en,
  },
  {
    code: "es",
    endonym: "Español",
    englishName: "Spanish",
    resources: es,
  },
  {
    code: "fr",
    endonym: "Français",
    englishName: "French",
    resources: fr,
  },
  {
    code: "it",
    endonym: "Italiano",
    englishName: "Italian",
    resources: it,
  },
];

export const DEFAULT_LANGUAGE = "en";

/** Exportierte Liste der registrierten Codes — Editionen prüfen damit, ob
 * sie für jede Sprache einen eigenen Namensraum haben. */
export const SUPPORTED_LANGUAGES: readonly string[] = LANGUAGE_REGISTRY.map((l) => l.code);
export type SupportedLanguage = string;

/** Registry nach englischem Namen sortiert (Reihenfolge im Picker). */
export function languagesForPicker(registry: readonly LanguageEntry[] = LANGUAGE_REGISTRY) {
  return [...registry].sort((a, b) => a.englishName.localeCompare(b.englishName, "en"));
}

export function isSupportedLanguage(
  value: string | null | undefined,
  codes: readonly string[] = SUPPORTED_LANGUAGES,
): value is string {
  return typeof value === "string" && codes.includes(value);
}

/** Bildet eine System-Locale (`pt-BR`, `zh_TW`, `nn-NO`, …) auf eine
 * registrierte Sprache ab; sonst Englisch. Chinesisch: vereinfacht ->
 * `zh-Hans`, traditionell wird nicht angeboten -> Fallback. */
export function languageForLocale(
  raw: string | null | undefined,
  codes: readonly string[] = SUPPORTED_LANGUAGES,
): string {
  const parts = (raw ?? "").trim().replace(/_/g, "-").split("-").filter(Boolean);
  const base = parts[0]?.toLowerCase();
  if (!base) return DEFAULT_LANGUAGE;
  const rest = parts.slice(1).map((p) => p.toLowerCase());
  const pick = (code: string) => (codes.includes(code) ? code : DEFAULT_LANGUAGE);
  switch (base) {
    case "pt":
      return pick("pt-BR");
    case "zh": {
      if (rest.includes("hans") && !rest.includes("hant")) return pick("zh-Hans");
      const traditional =
        rest.includes("hant") || rest.some((p) => p === "tw" || p === "hk" || p === "mo");
      return traditional ? DEFAULT_LANGUAGE : pick("zh-Hans");
    }
    case "no":
    case "nb":
    case "nn":
      return pick("nb");
    default:
      return pick(base);
  }
}
