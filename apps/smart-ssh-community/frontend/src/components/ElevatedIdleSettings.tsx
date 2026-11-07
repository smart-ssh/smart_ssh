import type { TranslationKey } from "../i18nTypes";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, getElevatedIdleTimeout, setElevatedIdleTimeout } from "../api/api";

/** Issue #1193: the allowed choices; `0` is "never". Must match the backend. */
const CHOICES = [5, 15, 30, 60, 0] as const;
const DEFAULT_MINUTES = 15;

const LABEL_KEY: Record<(typeof CHOICES)[number], TranslationKey> = {
  5: "elevatedIdle.minutes5",
  15: "elevatedIdle.minutes15",
  30: "elevatedIdle.minutes30",
  60: "elevatedIdle.minutes60",
  0: "elevatedIdle.never",
};

/** Spec 0067, A5: after this long without elevated activity the backend
 * switches the file browser's elevated rights off. */
export function ElevatedIdleSettings() {
  const { t } = useTranslation();
  const [minutes, setMinutes] = useState<number>(DEFAULT_MINUTES);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getElevatedIdleTimeout()
      .then(setMinutes)
      .catch((err) => setError(commandErrorMessage(err)));
  }, []);

  const handleChange = async (value: number) => {
    const previous = minutes;
    setMinutes(value);
    setError(null);
    try {
      await setElevatedIdleTimeout(value);
    } catch (err) {
      setMinutes(previous);
      setError(commandErrorMessage(err));
    }
  };

  return (
    <div className="mt-6">
      {error && (
        <p role="alert" className="mb-2 rounded bg-danger-subtle px-3 py-2 text-sm text-danger-fg-soft">
          {error}
        </p>
      )}
      <label className="flex items-center gap-2 text-sm text-fg-muted">
        {t("elevatedIdle.label")}
        <select
          value={minutes}
          onChange={(e) => handleChange(Number(e.target.value))}
          className="rounded border border-border-strong bg-surface-sunken px-2.5 py-1.5 text-sm text-fg"
        >
          {CHOICES.map((value) => (
            <option key={value} value={value}>
              {t(LABEL_KEY[value])}
            </option>
          ))}
        </select>
      </label>
      <p className="mt-1 text-xs text-fg-subtle">{t("elevatedIdle.hint")}</p>
    </div>
  );
}
