import type { TranslationKey } from "../i18nTypes";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { StatusSquare } from "./ui";
import {
  AUTO_RECONNECT_SECONDS,
  type AutoReconnectSeconds,
  DEFAULT_AUTO_RECONNECT_SECONDS,
  loadAutoReconnectSeconds,
  saveAutoReconnectSeconds,
} from "../settings/autoReconnectSettings";

const LABEL_KEY: Record<AutoReconnectSeconds, TranslationKey> = {
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
    <fieldset className="mt-6 border-t border-border pt-3">
      <legend className="text-label pr-2">{t("autoReconnect.label")}</legend>
      {error && (
        <p role="alert" className="mb-2 flex items-center gap-2 border border-danger-border bg-danger-subtle px-3 py-2 text-sm text-danger-fg-soft">
          <StatusSquare tone="danger" />{error}
        </p>
      )}
      <div className="flex items-center gap-2 text-sm text-fg-muted">
        <select
          aria-label={t("autoReconnect.label")}
          value={seconds}
          onChange={(e) => handleChange(Number(e.target.value) as AutoReconnectSeconds)}
          // Same look as `FIELD_CONTROL_CLASS`, but the native focus outline stays: WebKit
          // shows no ring-based focus style on a `<select>`.
          className="border border-border-strong bg-surface-sunken px-2.5 py-1.5 text-sm text-fg narrow:min-h-11"
        >
          {AUTO_RECONNECT_SECONDS.map((value) => (
            <option key={value} value={value}>
              {t(LABEL_KEY[value])}
            </option>
          ))}
        </select>
      </div>
    </fieldset>
  );
}
