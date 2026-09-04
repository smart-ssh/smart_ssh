import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, getChatSessionRetentionDays, setChatSessionRetentionDays } from "../api";

/** Spec 0034, Abschnitt 5: optionale Aufbewahrungsdauer für persistente
 * Chat-Sitzungen — Default "nie automatisch löschen" (`null`). Reines
 * Zahlenfeld statt Dropdown mit festen Stufen: die Spec nennt keine
 * vorgegebene Auswahl, ein freies Feld deckt jeden gewünschten Zeitraum ab. */
export function ChatRetentionSettings() {
  const { t } = useTranslation();
  const [days, setDays] = useState<number | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getChatSessionRetentionDays()
      .then((value) => {
        setDays(value);
        setLoaded(true);
      })
      .catch((err) => setError(commandErrorMessage(err)));
  }, []);

  const handleToggleEnabled = async (enabled: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const next = enabled ? 30 : null;
      await setChatSessionRetentionDays(next);
      setDays(next);
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const handleDaysChange = async (value: number) => {
    if (!Number.isFinite(value) || value < 1) return;
    setBusy(true);
    setError(null);
    try {
      await setChatSessionRetentionDays(value);
      setDays(value);
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  if (!loaded) return null;

  return (
    <div className="mb-6 border-t border-slate-700 pt-4">
      <h3 className="font-heading mb-2 text-sm font-semibold tracking-wide text-slate-200">
        {t("chatRetention.label")}
      </h3>
      {error && <p className="mb-2 rounded bg-red-950 px-2 py-1 text-xs text-red-300">{error}</p>}
      <label className="mb-2 flex items-center gap-2 text-sm text-slate-300">
        <input
          type="checkbox"
          checked={days !== null}
          disabled={busy}
          onChange={(e) => handleToggleEnabled(e.target.checked)}
        />
        {days === null
          ? t("chatRetention.neverDelete")
          : t("chatRetention.days", { count: days })}
      </label>
      {days !== null && (
        <input
          type="number"
          min={1}
          value={days}
          disabled={busy}
          onChange={(e) => handleDaysChange(e.target.valueAsNumber)}
          className="w-20 rounded border border-slate-600 bg-slate-900 px-1.5 py-0.5 text-sm text-slate-200"
        />
      )}
      <p className="mt-1 text-xs text-slate-500">{t("chatRetention.hint")}</p>
    </div>
  );
}
