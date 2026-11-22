import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AutoReconnectSettings } from "./AutoReconnectSettings";
import { ElevatedIdleSettings } from "./ElevatedIdleSettings";
import { FIELD_CONTROL_CLASS, StatusSquare } from "./ui";
import {
  commandErrorMessage,
  getChatSessionRetentionDays,
  getRecordActiveMinutes,
  setChatSessionRetentionDays,
  setRecordActiveMinutes,
} from "../api/api";

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
  const [recordActive, setRecordActive] = useState(true);

  useEffect(() => {
    getChatSessionRetentionDays()
      .then((value) => {
        setDays(value);
        setLoaded(true);
      })
      .catch((err) => setError(commandErrorMessage(err)));
    getRecordActiveMinutes()
      .then(setRecordActive)
      .catch((err) => setError(commandErrorMessage(err)));
  }, []);

  const handleToggleRecordActive = async (value: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await setRecordActiveMinutes(value);
      setRecordActive(value);
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

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

  // Spec 0055, Teil 3: `SettingsScreen` rendert den Sektions-Titel bereits
  // selbst (`{active?.label}`) — die eigene `<h3>` hier war eine doppelte
  // Überschrift. Mit ihr fällt auch der `border-t`/`pt-4`-Rahmen weg: der
  // war für eine unterhalb von anderem Inhalt angehängte Sektion gedacht
  // (vor Spec 0050 lag das hier direkt unter `AiProviderSettings`), diese
  // Sektion ist seit Spec 0050 aber die ALLEINIGE Kategorie-Inhalts-
  // Komponente — ein oberer Trennstrich direkt unter dem
  // SettingsScreen-eigenen Header-Rahmen sähe wie eine doppelte Linie aus.
  return (
    <div>
      {error && <p role="alert" className="mb-2 flex items-center gap-2 border border-danger-border bg-danger-subtle px-3 py-2 text-sm text-danger-fg-soft"><StatusSquare tone="danger" />{error}</p>}
      {/* Spec-Reviewer-Fund (Spec 0055, Review des Gesamtpakets): die
       * entfernte `<h3>` (Teil 3) war NICHT nur eine doppelte Kategorie-
       * Überschrift, sondern trug auch die einzige Feldbeschriftung ("Chat-
       * Verlauf aufbewahren für") — ohne sie fehlte die Angabe, WOFÜR die
       * Tage-Checkbox/-Zahl unten gelten. Als schlichter Absatz statt einer
       * Überschrift wieder eingesetzt (kein erneutes Duplikat des
       * Kategorie-Titels "Sitzungen & Daten", der weiterhin ausschließlich
       * von `SettingsScreen` gerendert wird). */}
      <fieldset className="border-t border-border pt-3">
      <legend className="text-label pr-2">{t("chatRetention.label")}</legend>
      <label className="mb-2 flex items-center gap-3 text-sm text-fg-muted narrow:min-h-11">
        <input
          type="checkbox"
          className="switch"
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
          aria-label={t("chatRetention.label")}
          className={`${FIELD_CONTROL_CLASS} w-24`}
        />
      )}
      <p className="mt-1 text-xs text-fg-subtle">{t("chatRetention.hint")}</p>
      </fieldset>
      <div className="mt-3 border-t border-border pt-3">
        <label className="mb-1 flex items-center gap-3 text-sm text-fg-muted narrow:min-h-11">
          <input
            type="checkbox"
            className="switch"
            checked={recordActive}
            disabled={busy}
            onChange={(e) => handleToggleRecordActive(e.target.checked)}
          />
          {t("recordActiveMinutes.label")}
        </label>
        <p className="text-xs text-fg-subtle">{t("recordActiveMinutes.hint")}</p>
      </div>
      <AutoReconnectSettings />
      <ElevatedIdleSettings />
    </div>
  );
}
