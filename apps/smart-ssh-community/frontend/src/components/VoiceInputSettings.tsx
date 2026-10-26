import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode } from "../api";
import {
  cancelSpeechModelDownload,
  deleteSpeechModel,
  getSelectedSpeechModel,
  getSpeechLanguage,
  getSpeechModelStatus,
  onSpeechModelStatus,
  SPEECH_MODEL_SIZE_MIB,
  SPEECH_MODEL_SOURCE,
  SPEECH_MODELS,
  setSelectedSpeechModel,
  setSpeechLanguage,
  startSpeechModelDownload,
  type SpeechModelId,
  type SpeechModelStatus,
} from "../voiceApi";
import {
  loadVoiceInputEnabled,
  saveVoiceInputEnabled,
  voiceErrorKey,
} from "../voiceInputSettings";
import { Button } from "./ui";

/** ISO codes offered for a fixed recognition language (all supported by Whisper). */
const LANGUAGE_CODES = [
  "en", "de", "es", "fr", "it", "pt", "nl", "pl", "cs", "ro", "hu", "el", "tr", "ru", "uk",
  "da", "sv", "no", "fi", "id", "vi", "hi", "ja", "ko", "zh",
] as const;

/** Spec 0108 §2, §5.3, §5.4: on/off, model, recognition language. */
export function VoiceInputSettings() {
  const { t, i18n } = useTranslation();
  const [enabled, setEnabled] = useState(true);
  const [model, setModel] = useState<SpeechModelId>("base");
  const [statuses, setStatuses] = useState<Partial<Record<SpeechModelId, SpeechModelStatus>>>({});
  const [language, setLanguage] = useState("auto");
  const [errorKey, setErrorKey] = useState<string | null>(null);

  const fail = (err: unknown) => setErrorKey(voiceErrorKey(commandErrorCode(err)));

  const refresh = useCallback(async (id: SpeechModelId) => {
    const status = await getSpeechModelStatus(id);
    setStatuses((prev) => ({ ...prev, [id]: status }));
  }, []);

  useEffect(() => {
    loadVoiceInputEnabled().then(setEnabled).catch(() => {});
    getSelectedSpeechModel().then(setModel).catch(fail);
    getSpeechLanguage().then(setLanguage).catch(fail);
    for (const id of SPEECH_MODELS) refresh(id).catch(fail);
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onSpeechModelStatus((ev) => setStatuses((prev) => ({ ...prev, [ev.model]: ev })))
      .then((u) => (disposed ? u() : (unlisten = u)))
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh]);

  const names = new Intl.DisplayNames([i18n.language], { type: "language" });
  const languageName = (code: string) => {
    try {
      return names.of(code) ?? code;
    } catch {
      return code;
    }
  };

  const run = (action: () => Promise<unknown>) => {
    setErrorKey(null);
    action().catch(fail);
  };

  const statusText = (status: SpeechModelStatus | undefined) => {
    switch (status?.state) {
      case "ready":
        return t("voice.settings.statusReady");
      case "downloading":
        return t("voice.settings.statusDownloading", {
          percent: status.total > 0 ? Math.floor((status.done / status.total) * 100) : 0,
        });
      case "error":
        return t(voiceErrorKey(status.key));
      default:
        return t("voice.settings.statusNotDownloaded");
    }
  };

  return (
    <div className="space-y-6">
      <p className="text-sm text-fg-muted">
        {t("voice.settings.description", { source: SPEECH_MODEL_SOURCE })}
      </p>
      {errorKey && (
        <p role="alert" className="rounded bg-danger-subtle px-3 py-2 text-sm text-danger-fg-soft">
          {t(errorKey)}
        </p>
      )}
      <label className="flex items-center gap-2 text-sm text-fg-muted">
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => {
            const value = e.target.checked;
            setEnabled(value);
            run(() => saveVoiceInputEnabled(value).catch((err) => {
              setEnabled(!value);
              throw err;
            }));
          }}
        />
        {t("voice.settings.enabled")}
      </label>

      <fieldset className="space-y-3">
        <legend className="mb-2 text-sm font-semibold text-fg-soft">
          {t("voice.settings.model")}
        </legend>
        {SPEECH_MODELS.map((id) => {
          const status = statuses[id];
          const downloading = status?.state === "downloading";
          const ready = status?.state === "ready";
          return (
            <div key={id} className="flex flex-wrap items-center gap-3 text-sm text-fg-muted">
              <label className="flex items-center gap-2">
                <input
                  type="radio"
                  name="speech-model"
                  checked={model === id}
                  onChange={() => {
                    setModel(id);
                    run(() => setSelectedSpeechModel(id));
                  }}
                />
                {t(`voice.settings.models.${id}`, { size: SPEECH_MODEL_SIZE_MIB[id] })}
              </label>
              <span aria-live="polite" className="text-fg-subtle">
                {statusText(status)}
              </span>
              {downloading ? (
                <Button size="sm" onClick={() => run(() => cancelSpeechModelDownload(id))}>
                  {t("voice.consent.cancel")}
                </Button>
              ) : ready ? (
                <Button
                  size="sm"
                  variant="danger"
                  onClick={() => run(async () => {
                    await deleteSpeechModel(id);
                    await refresh(id);
                  })}
                >
                  {t("voice.settings.delete")}
                </Button>
              ) : (
                <Button
                  size="sm"
                  variant="primary"
                  onClick={() => run(() => startSpeechModelDownload(id))}
                >
                  {t("voice.settings.download")}
                </Button>
              )}
            </div>
          );
        })}
      </fieldset>

      <label className="flex items-center gap-2 text-sm text-fg-muted">
        {t("voice.settings.language")}
        <select
          value={language}
          onChange={(e) => {
            const previous = language;
            setLanguage(e.target.value);
            run(() => setSpeechLanguage(e.target.value).catch((err) => {
              setLanguage(previous);
              throw err;
            }));
          }}
          className="rounded border border-border-strong bg-surface-sunken px-2.5 py-1.5 text-sm text-fg"
        >
          <option value="auto">{t("voice.settings.languageAuto")}</option>
          {LANGUAGE_CODES.map((code) => (
            <option key={code} value={code}>
              {languageName(code)}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
