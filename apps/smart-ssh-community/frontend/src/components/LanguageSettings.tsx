import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage } from "../api";
import { setLanguage, type SupportedLanguage } from "../i18n";
import { languagesForPicker } from "../languages";
import { Button } from "./ui";

/** Spec 0050, Teil 1: aus `AiProviderSettings` herausgelöster
 * Sprachumschalter — eigene Kategorie ("Anzeige & Sprache") in der
 * zweispaltigen Settings-Struktur statt Teil der KI-Provider-Sektion. */
export function LanguageSettings() {
  const { i18n } = useTranslation();
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
      {error && <p role="alert" className="mb-4 rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>}
      <div className="flex flex-wrap gap-2">
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
      </div>
    </div>
  );
}
