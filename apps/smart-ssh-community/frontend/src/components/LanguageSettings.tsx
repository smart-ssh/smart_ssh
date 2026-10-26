import { useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage } from "../api";
import { setLanguage, type SupportedLanguage } from "../i18n";
import { languagesForPicker } from "../languages";
import { THEME_CHOICES, getThemeChoice, setThemeChoice, subscribeTheme, type ThemeChoice } from "../theme";
import { Button } from "./ui";

/** Spec 0050, Teil 1: aus `AiProviderSettings` herausgelöster
 * Sprachumschalter — eigene Kategorie ("Anzeige & Sprache") in der
 * zweispaltigen Settings-Struktur statt Teil der KI-Provider-Sektion. */
export function LanguageSettings() {
  const { i18n, t } = useTranslation();
  const themeChoice = useSyncExternalStore(subscribeTheme, getThemeChoice);
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
  /** Issue #804: applies immediately; persistence errors are shown, the choice stays applied. */
  const handleThemeChange = (choice: ThemeChoice) => {
    setThemeChoice(choice).catch((err) => setError(commandErrorMessage(err)));
  };

  return (
    <div>
      {error && <p role="alert" className="mb-4 rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>}
      <section aria-labelledby="appearance-heading" className="mb-6">
        <p id="appearance-heading" className="mb-2 text-sm font-semibold text-slate-200">
          {t("appearance.heading")}
        </p>
        <div className="flex flex-wrap gap-2">
          {THEME_CHOICES.map((choice) => (
            <Button
              key={choice}
              variant={themeChoice === choice ? "primary" : "secondary"}
              onClick={() => handleThemeChange(choice)}
              aria-pressed={themeChoice === choice}
            >
              {t(`appearance.${choice}`)}
            </Button>
          ))}
        </div>
      </section>
      <p id="language-heading" className="mb-2 text-sm font-semibold text-slate-200">
        {t("appearance.language")}
      </p>
      <fieldset aria-labelledby="language-heading" className="m-0 flex min-w-0 flex-wrap gap-2 border-0 p-0">
        {languagesForPicker().map(({ code: language, endonym }) => (
          <Button
            lang={language}
            key={language}
            variant={i18n.language === language ? "primary" : "secondary"}
            onClick={() => handleLanguageChange(language)}
            aria-pressed={i18n.language === language}
          >
            {endonym}
          </Button>
        ))}
      </fieldset>
    </div>
  );
}
