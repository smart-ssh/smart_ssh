import type { TranslationKey } from "../i18nTypes";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode } from "../api/api";
import { useActivities } from "../hooks/useActivities";
import {
  cancelSpeechModelDownload,
  deleteSpeechModel,
  getSelectedSpeechModel,
  getSpeechLanguage,
  getSpeechModelStatus,
  onSpeechModelStatus,
  listSpeechModels,
  SPEECH_MODEL_PARAKEET,
  speechModelSizeMib,
  setSelectedSpeechModel,
  setSpeechLanguage,
  startSpeechModelDownload,
  type SpeechModelId,
  type SpeechModelInfo,
  type SpeechModelStatus,
} from "../api/voiceApi";
import {
  loadVoiceInputEnabled,
  saveVoiceInputEnabled,
  voiceErrorKey,
} from "../settings/voiceInputSettings";
import { Button } from "./ui";

/** ISO codes offered for a fixed recognition language (all supported by Whisper). */
const LANGUAGE_CODES = [
  "en", "de", "es", "fr", "it", "pt", "nl", "pl", "cs", "ro", "hu", "el", "tr", "ru", "uk",
  "da", "sv", "no", "fi", "id", "vi", "hi", "ja", "ko", "zh",
] as const;

/** Kind of the activity of a speech model download; its key is the model id. */
const SPEECH_MODEL_ACTIVITY_KIND = "speech-model";

/** Spec 0108 §2, §5.3, §5.4: on/off, model, recognition language. */
export function VoiceInputSettings() {
  const { t, i18n } = useTranslation();
  const [enabled, setEnabled] = useState(true);
  const [model, setModel] = useState<SpeechModelId>("base");
  const [models, setModels] = useState<SpeechModelInfo[]>([]);
  const [statuses, setStatuses] = useState<Partial<Record<SpeechModelId, SpeechModelStatus>>>({});
  const [language, setLanguage] = useState("auto");
  const [errorKey, setErrorKey] = useState<TranslationKey | null>(null);

  // Issue #1326: a running download is also an activity; a reopened dialog
  // shows it (even before the status call answers) and never starts a second.
  const runningDownloads = useActivities().filter(
    (a) => a.kind === SPEECH_MODEL_ACTIVITY_KIND && a.state === "running",
  );

  const fail = (err: unknown) => setErrorKey(voiceErrorKey(commandErrorCode(err)));

  const refresh = useCallback(async (id: SpeechModelId) => {
    const status = await getSpeechModelStatus(id);
    setStatuses((prev) => ({ ...prev, [id]: status }));
  }, []);

  useEffect(() => {
    loadVoiceInputEnabled().then(setEnabled).catch(() => {});
    getSelectedSpeechModel().then(setModel).catch(fail);
    getSpeechLanguage().then(setLanguage).catch(fail);
    listSpeechModels()
      .then((list) => {
        setModels(list);
        for (const m of list) refresh(m.id).catch(fail);
      })
      .catch(fail);
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
        {t("voice.settings.description", { source: models[0]?.source_host ?? "huggingface.co" })}
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
        {models.map((info) => {
          const id = info.id;
          const activity = runningDownloads.find((a) => a.key === id);
          // The activity wins over a not-yet-loaded status (reopened dialog).
          const status: SpeechModelStatus | undefined =
            activity && statuses[id]?.state !== "downloading"
              ? { state: "downloading", done: activity.done, total: activity.total ?? 0 }
              : statuses[id];
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
                {t(`voice.settings.models.${id}` as TranslationKey, { size: speechModelSizeMib(info) })}
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
      {model === SPEECH_MODEL_PARAKEET && models.some((m) => m.id === SPEECH_MODEL_PARAKEET) && (
        <p className="text-xs text-fg-subtle">{t("voice.settings.languageParakeetHint")}</p>
      )}
    </div>
  );
}
