import { useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage } from "../api/api";
import { setLanguage, type SupportedLanguage } from "../i18n";
import { languagesForPicker } from "../lib/languages";
import { THEME_CHOICES, getThemeChoice, setThemeChoice, subscribeTheme, type ThemeChoice } from "../lib/theme";
import { Button } from "./ui";

/** Segmented option: flat, 1 px rule, 2 px accent rule under the selected one. */
const segmentClass = (selected: boolean) =>
  `border border-b-2 px-3 text-[13px] ${selected ? "border-b-accent border-x-border-strong border-t-border-strong bg-surface-raised text-fg-strong" : "border-border-strong bg-transparent text-fg-muted hover:bg-surface-hover"}`;

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
      {error && <p role="alert" className="mb-4 border-l-2 border-danger bg-danger-subtle px-3 py-2 text-sm text-danger-fg-soft">{error}</p>}
      <section aria-labelledby="appearance-heading" className="mb-6">
        <p id="appearance-heading" className="mb-2 text-sm font-semibold text-fg-soft">
          {t("appearance.heading")}
        </p>
        <div className="flex flex-wrap gap-px">
          {THEME_CHOICES.map((choice) => (
            <Button
              key={choice}
              variant="bare"
              className={`label-condensed inline-flex h-8 items-center justify-center focus-ring-flush narrow:min-h-11 ${segmentClass(themeChoice === choice)}`}
              onClick={() => handleThemeChange(choice)}
              aria-pressed={themeChoice === choice}
            >
              {t(`appearance.${choice}`)}
            </Button>
          ))}
        </div>
      </section>
      <p id="language-heading" className="mb-2 text-sm font-semibold text-fg-soft">
        {t("appearance.language")}
      </p>
      <fieldset aria-labelledby="language-heading" className="m-0 flex min-w-0 flex-wrap gap-px border-0 p-0">
        {languagesForPicker().map(({ code: language, endonym }) => (
          <Button
            lang={language}
            key={language}
            variant="bare"
            className={`inline-flex h-8 items-center justify-center focus-ring-flush narrow:min-h-11 ${segmentClass(i18n.language === language)}`}
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
