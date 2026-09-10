import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage } from "../api";
import { setLanguage, SUPPORTED_LANGUAGES, type SupportedLanguage } from "../i18n";

/** Spec 0050, Teil 1: aus `AiProviderSettings` herausgelöster
 * Sprachumschalter — eigene Kategorie ("Anzeige & Sprache") in der
 * zweispaltigen Settings-Struktur statt Teil der KI-Provider-Sektion. */
export function LanguageSettings() {
  const { t, i18n } = useTranslation();
  const [error, setError] = useState<string | null>(null);

  /** Spec 0024, Abschnitt 4: Wirkung sofort ohne Neustart —
   * `setLanguage` ruft `i18next.changeLanguage` auf, das automatisch alle
   * `useTranslation`-Verbraucher (inkl. dieser Komponente) neu rendert. */
  const handleLanguageChange = (language: SupportedLanguage) => {
    setLanguage(language).catch((err) => setError(commandErrorMessage(err)));
  };

  // Spec 0055, Teil 3: `SettingsScreen` rendert den Sektions-Titel bereits
  // selbst ("Anzeige & Sprache") — dieselbe doppelte Überschrift wie in
  // `ChatRetentionSettings.tsx`/`McpServerSettings.tsx`.
  return (
    <div>
      {error && <p className="mb-4 rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>}
      <div className="flex gap-2">
        {SUPPORTED_LANGUAGES.map((language) => (
          <button
            key={language}
            type="button"
            onClick={() => handleLanguageChange(language)}
            aria-pressed={i18n.language === language}
            className={`rounded px-3 py-1.5 text-sm ${
              i18n.language === language
                ? "bg-indigo-600 text-white"
                : "bg-slate-700 text-slate-200 hover:bg-slate-600"
            }`}
          >
            {t(`settings.language.${language}`)}
          </button>
        ))}
      </div>
    </div>
  );
}
