import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, openLogDirectory } from "../api";

/** Spec 0050, Teil 1: aus `AiProviderSettings` herausgelöst — eigene
 * Kategorie ("Diagnose") in der zweispaltigen Settings-Struktur. */
export function DiagnosticsSettings() {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);

  /** Spec 0016, Abschnitt 5: ein Klick statt manuell zum
   * plattformspezifischen Log-Ordner navigieren zu müssen. */
  const handleOpenLogDirectory = async () => {
    setError(null);
    try {
      await openLogDirectory();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  return (
    <div>
      {error && <p className="mb-4 rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>}
      <button
        type="button"
        onClick={handleOpenLogDirectory}
        className="w-full rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-700"
      >
        {t("aiProvider.openLogs")}
      </button>
    </div>
  );
}
