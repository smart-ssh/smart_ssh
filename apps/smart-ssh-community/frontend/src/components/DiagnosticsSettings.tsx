import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commandErrorMessage,
  generateDiagnosticsBundle,
  openLogDirectory,
  saveDiagnosticsBundle,
} from "../api";

/** Spec 0050, Teil 1: aus `AiProviderSettings` herausgelöst — eigene
 * Kategorie ("Diagnose") in der zweispaltigen Settings-Struktur. */
export function DiagnosticsSettings() {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);
  const [bundle, setBundle] = useState<string | null>(null);
  const [generating, setGenerating] = useState(false);

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

  /** Spec 0063 §3: nur erzeugen + zur Durchsicht anzeigen — kein
   * automatisches Speichern/Versenden. Der Text kommt bereits redigiert
   * vom Backend (`generate_diagnostics_bundle`); diese Vorschau ist die
   * zweite, für den Nutzer sichtbare Verteidigungslinie (Spec 0063 §3,
   * letzter Punkt), falls der Redactor ein Muster nicht kennt. */
  const handleGenerateBundle = async () => {
    setError(null);
    setGenerating(true);
    try {
      const content = await generateDiagnosticsBundle();
      setBundle(content);
    } catch (err) {
      setError(t("diagnostics.generateFailed") + " " + commandErrorMessage(err));
    } finally {
      setGenerating(false);
    }
  };

  const handleSaveBundle = async () => {
    if (!bundle) return;
    setError(null);
    try {
      await saveDiagnosticsBundle(bundle);
    } catch (err) {
      setError(t("diagnostics.saveFailed") + " " + commandErrorMessage(err));
    }
  };

  return (
    <div className="space-y-4">
      {error && <p className="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>}
      <button
        type="button"
        onClick={handleOpenLogDirectory}
        className="w-full rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-700"
      >
        {t("aiProvider.openLogs")}
      </button>
      <button
        type="button"
        onClick={handleGenerateBundle}
        disabled={generating}
        className="w-full rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-700 disabled:opacity-60"
      >
        {generating ? t("diagnostics.generating") : t("diagnostics.generateBundle")}
      </button>
      {bundle && (
        <div className="space-y-2">
          <p className="text-xs text-slate-400">{t("diagnostics.previewHint")}</p>
          <textarea
            readOnly
            value={bundle}
            className="h-64 w-full select-text rounded border border-slate-600 bg-slate-900 p-2 font-mono text-xs text-slate-100"
          />
          <button
            type="button"
            onClick={handleSaveBundle}
            className="w-full rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-700"
          >
            {t("diagnostics.save")}
          </button>
        </div>
      )}
    </div>
  );
}
