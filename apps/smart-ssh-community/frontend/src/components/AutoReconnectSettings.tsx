import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AUTO_RECONNECT_SECONDS,
  type AutoReconnectSeconds,
  DEFAULT_AUTO_RECONNECT_SECONDS,
  loadAutoReconnectSeconds,
  saveAutoReconnectSeconds,
} from "../autoReconnectSettings";

const LABEL_KEY: Record<AutoReconnectSeconds, string> = {
  0: "autoReconnect.off",
  30: "autoReconnect.seconds30",
  60: "autoReconnect.seconds60",
  300: "autoReconnect.minutes5",
};

/** Issue #356, Spec 0107 §8: wie lange nach einem Verbindungsabbruch
 * automatisch neu verbunden wird. Wirkt ab dem nächsten Abbruch. */
export function AutoReconnectSettings() {
  const { t } = useTranslation();
  const [seconds, setSeconds] = useState<AutoReconnectSeconds>(DEFAULT_AUTO_RECONNECT_SECONDS);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadAutoReconnectSeconds()
      .then(setSeconds)
      .catch((err) => setError(String(err)));
  }, []);

  const handleChange = async (value: AutoReconnectSeconds) => {
    const previous = seconds;
    setSeconds(value);
    setError(null);
    try {
      await saveAutoReconnectSeconds(value);
    } catch (err) {
      setSeconds(previous);
      setError(String(err));
    }
  };

  return (
    <div className="mt-6">
      {error && <p className="mb-2 rounded bg-red-950 px-2 py-1 text-xs text-red-300">{error}</p>}
      <label className="flex items-center gap-2 text-sm text-slate-300">
        {t("autoReconnect.label")}
        <select
          value={seconds}
          onChange={(e) => handleChange(Number(e.target.value) as AutoReconnectSeconds)}
          className="rounded border border-slate-600 bg-slate-900 px-1.5 py-0.5 text-sm text-slate-200"
        >
          {AUTO_RECONNECT_SECONDS.map((value) => (
            <option key={value} value={value}>
              {t(LABEL_KEY[value])}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
