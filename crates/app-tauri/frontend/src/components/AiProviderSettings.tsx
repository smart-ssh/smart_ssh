import { type FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  addAiProvider,
  commandErrorMessage,
  deleteAiProvider,
  discoverModels,
  fetchAttestationInfo,
  listAiProviders,
  openLogDirectory,
  setActiveAiProvider,
} from "../api";
import { setLanguage, SUPPORTED_LANGUAGES, type SupportedLanguage } from "../i18n";
import { loadRiskClassifierSettings, saveRiskClassifierSettings } from "../riskSettings";
import { McpServerSettings } from "./McpServerSettings";
import {
  type AiProviderConfigDto,
  type AiProviderConfigInput,
  type ProviderType,
  PROVIDER_TYPE_LABELS,
  needsBaseUrl,
  supportsModelDiscovery,
} from "../types";

const PROVIDER_TYPES: ProviderType[] = [
  "openai",
  "anthropic",
  "generic_openai_compatible",
  "ollama",
];

function emptyForm(): AiProviderConfigInput {
  return {
    providerType: "openai",
    displayName: "",
    baseUrl: null,
    model: "",
    supportsNativeToolCalling: true,
    apiKey: "",
    extraHeaders: [],
    attestationUrl: null,
  };
}

const MODEL_DATALIST_ID = "ai-provider-model-options";

interface AiProviderSettingsProps {
  onClose: () => void;
  /** Löst neu laden von `list_ai_providers` im Elternscreen aus (z. B. für
   * den "kein Provider konfiguriert"-Hinweis), sobald sich hier etwas
   * ändert — kein globaler State-Store in Teil 1, dafür reicht ein
   * simpler Callback. */
  onProvidersChanged: () => void;
}

export function AiProviderSettings({ onClose, onProvidersChanged }: AiProviderSettingsProps) {
  const { t, i18n } = useTranslation();
  const [providers, setProviders] = useState<AiProviderConfigDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<AiProviderConfigInput>(emptyForm());
  const [submitting, setSubmitting] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsFailed, setModelsFailed] = useState(false);
  const [attestationResults, setAttestationResults] = useState<Record<string, string>>({});
  const [attestationLoading, setAttestationLoading] = useState<Record<string, boolean>>({});
  const [attestationErrors, setAttestationErrors] = useState<Record<string, string>>({});
  const [riskClassifierEnabled, setRiskClassifierEnabled] = useState(false);
  const [riskClassifierProviderId, setRiskClassifierProviderId] = useState<string | null>(null);
  const [riskSettingsSaving, setRiskSettingsSaving] = useState(false);

  useEffect(() => {
    loadRiskClassifierSettings()
      .then((settings) => {
        setRiskClassifierEnabled(settings.enabled);
        setRiskClassifierProviderId(settings.providerId);
      })
      .catch((err) => setError(commandErrorMessage(err)));
  }, []);

  /** Spec 0026, Abschnitt 3, Punkt 1: erst bei der nächsten `connect()`
   * wirksam (s. `riskSettings.ts`-Doc-Kommentar) — trotzdem sofort
   * gespeichert, damit die Einstellung nicht verloren geht. */
  const handleRiskClassifierChange = async (enabled: boolean, providerId: string | null) => {
    setRiskClassifierEnabled(enabled);
    setRiskClassifierProviderId(providerId);
    setRiskSettingsSaving(true);
    try {
      await saveRiskClassifierSettings({ enabled, providerId });
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setRiskSettingsSaving(false);
    }
  };

  /** Spec 0024, Abschnitt 4: Wirkung sofort ohne Neustart —
   * `setLanguage` ruft `i18next.changeLanguage` auf, das automatisch alle
   * `useTranslation`-Verbraucher (inkl. dieser Komponente) neu rendert. */
  const handleLanguageChange = (language: SupportedLanguage) => {
    setLanguage(language).catch((err) => setError(commandErrorMessage(err)));
  };

  const reload = () => {
    listAiProviders()
      .then(setProviders)
      .catch((err) => setError(commandErrorMessage(err)));
  };

  useEffect(reload, []);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const newId = await addAiProvider(form);
      // Spec 0025, Abschnitt 4: "beim Speichern ... abrufen" — automatisch,
      // wenn eine Attestierungs-URL hinterlegt wurde.
      if (form.attestationUrl) {
        void handleFetchAttestation(newId);
      }
      setForm(emptyForm());
      setModels([]);
      setModelsFailed(false);
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  };

  /** Spec 0025, Abschnitt 2: läuft mit den aktuellen Formulardaten, auch
   * bevor der Provider gespeichert ist — schlägt der Aufruf fehl (nicht
   * jeder Anbieter unterstützt den Endpunkt zuverlässig), bleibt das
   * Modellfeld einfach ein normales Freitextfeld (kein `setError`, kein
   * blockierender Zustand). */
  const handleDiscoverModels = async () => {
    setModelsLoading(true);
    setModelsFailed(false);
    try {
      const discovered = await discoverModels(form);
      setModels(discovered);
    } catch {
      setModelsFailed(true);
      setModels([]);
    } finally {
      setModelsLoading(false);
    }
  };

  /** Spec 0025, Abschnitt 4: "auf Wunsch erneut" abrufbar — pro
   * Provider-Zeile in der Liste nutzbar, nicht nur direkt nach dem
   * Speichern. */
  const handleFetchAttestation = async (providerId: string) => {
    setAttestationLoading((prev) => ({ ...prev, [providerId]: true }));
    setAttestationErrors((prev) => ({ ...prev, [providerId]: "" }));
    try {
      const info = await fetchAttestationInfo(providerId);
      setAttestationResults((prev) => ({ ...prev, [providerId]: info }));
    } catch (err) {
      setAttestationErrors((prev) => ({ ...prev, [providerId]: commandErrorMessage(err) }));
    } finally {
      setAttestationLoading((prev) => ({ ...prev, [providerId]: false }));
    }
  };

  const updateExtraHeader = (index: number, key: string, value: string) => {
    const next = [...form.extraHeaders];
    next[index] = [key, value];
    setForm({ ...form, extraHeaders: next });
  };

  const addExtraHeader = () => setForm({ ...form, extraHeaders: [...form.extraHeaders, ["", ""]] });

  const removeExtraHeader = (index: number) =>
    setForm({ ...form, extraHeaders: form.extraHeaders.filter((_, i) => i !== index) });

  const handleDelete = async (id: string) => {
    setError(null);
    try {
      await deleteAiProvider(id);
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  const handleSetActive = async (id: string) => {
    setError(null);
    try {
      await setActiveAiProvider(id);
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

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
    <div className="fixed inset-0 flex items-center justify-center bg-black/50 p-4">
      <div className="max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-lg bg-slate-800 p-6 shadow-xl">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="font-heading text-lg font-semibold tracking-wide text-slate-100">
            {t("aiProvider.title")}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="text-slate-400 hover:text-slate-100"
            aria-label={t("common.close")}
          >
            ✕
          </button>
        </div>

        {error && (
          <p className="mb-4 rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
        )}

        <div className="mb-6">
          <h3 className="font-heading mb-2 text-sm font-semibold tracking-wide text-slate-200">
            {t("settings.language.label")}
          </h3>
          <div className="flex gap-2">
            {SUPPORTED_LANGUAGES.map((language) => (
              <button
                key={language}
                type="button"
                onClick={() => handleLanguageChange(language)}
                aria-pressed={i18n.language === language}
                className={`rounded px-3 py-1.5 text-sm ${
                  i18n.language === language
                    ? "bg-indigo-600 text-white"
                    : "bg-slate-700 text-slate-200 hover:bg-slate-600"
                }`}
              >
                {t(`settings.language.${language}`)}
              </button>
            ))}
          </div>
        </div>

        <button
          type="button"
          onClick={handleOpenLogDirectory}
          className="mb-6 w-full rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-700"
        >
          {t("aiProvider.openLogs")}
        </button>

        <ul className="mb-6 divide-y divide-slate-700 rounded-md border border-slate-700">
          {providers.length === 0 && (
            <li className="px-4 py-3 text-sm text-slate-400">{t("aiProvider.noProviders")}</li>
          )}
          {providers.map((provider) => (
            <li key={provider.id} className="px-4 py-3">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <p className="font-medium text-slate-100">
                    {provider.displayName}
                    {provider.isActive && (
                      <span className="ml-2 rounded bg-emerald-900 px-2 py-0.5 text-xs text-emerald-300">
                        {t("aiProvider.active")}
                      </span>
                    )}
                  </p>
                  <p className="text-sm text-slate-400">
                    {PROVIDER_TYPE_LABELS[provider.providerType]} · {provider.model}
                  </p>
                </div>
                <div className="flex shrink-0 gap-2">
                  {!provider.isActive && (
                    <button
                      type="button"
                      onClick={() => handleSetActive(provider.id)}
                      className="rounded bg-slate-700 px-2 py-1 text-xs text-slate-100 hover:bg-slate-600"
                    >
                      {t("aiProvider.setActive")}
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => handleDelete(provider.id)}
                    className="rounded bg-red-900 px-2 py-1 text-xs text-red-200 hover:bg-red-800"
                  >
                    {t("common.delete")}
                  </button>
                </div>
              </div>

              {provider.attestationUrl && (
                <div className="mt-2 space-y-1.5">
                  <button
                    type="button"
                    onClick={() => handleFetchAttestation(provider.id)}
                    disabled={attestationLoading[provider.id]}
                    className="rounded border border-slate-600 px-2 py-1 text-xs text-slate-300 hover:bg-slate-700 disabled:opacity-50"
                  >
                    {attestationLoading[provider.id]
                      ? t("aiProvider.attestationFetching")
                      : t("aiProvider.attestationFetch")}
                  </button>
                  {attestationErrors[provider.id] && (
                    <p className="text-xs text-red-400">{attestationErrors[provider.id]}</p>
                  )}
                  {attestationResults[provider.id] && (
                    <div className="space-y-1">
                      <p className="text-xs text-amber-300">
                        {t("aiProvider.attestationDisclaimerBeforeNot")}
                        <strong>{t("aiProvider.attestationDisclaimerNot")}</strong>
                        {t("aiProvider.attestationDisclaimerAfterNot")}
                      </p>
                      <p className="text-xs font-semibold text-slate-400">
                        {t("aiProvider.attestationResultLabel")}
                      </p>
                      <pre className="max-h-40 overflow-auto rounded border border-slate-700 bg-slate-950 p-2 text-xs whitespace-pre-wrap text-slate-300">
                        {attestationResults[provider.id]}
                      </pre>
                    </div>
                  )}
                </div>
              )}
            </li>
          ))}
        </ul>

        {/* Spec 0026, Abschnitt 3, Punkt 1: eigener Abschnitt für die
         * optionale KI-Zweitmeinung zur Daten-Risiko-Achse — standardmäßig
         * deaktiviert (Opt-in), separat wählbarer Provider, Hinweis auf ein
         * empfohlenes lokales Modell. */}
        <div className="mb-6 border-t border-slate-700 pt-4">
          <h3 className="font-heading mb-2 text-sm font-semibold tracking-wide text-slate-200">
            {t("aiProvider.riskClassifierTitle")}
          </h3>
          <p className="mb-2 text-xs text-slate-500">{t("aiProvider.riskClassifierHint")}</p>
          <label className="mb-2 flex items-center gap-2 text-sm text-slate-300">
            <input
              type="checkbox"
              checked={riskClassifierEnabled}
              onChange={(e) =>
                handleRiskClassifierChange(e.target.checked, riskClassifierProviderId)
              }
            />
            {t("aiProvider.riskClassifierEnable")}
          </label>
          {riskClassifierEnabled && (
            <label className="block text-sm text-slate-300">
              {t("aiProvider.riskClassifierProvider")}
              <select
                value={riskClassifierProviderId ?? ""}
                onChange={(e) => handleRiskClassifierChange(true, e.target.value || null)}
                disabled={riskSettingsSaving}
                className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
              >
                <option value="">{t("aiProvider.riskClassifierNoProvider")}</option>
                {providers.map((provider) => (
                  <option key={provider.id} value={provider.id}>
                    {provider.displayName}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>

        <McpServerSettings />

        <form onSubmit={handleSubmit} className="space-y-3">
          <h3 className="font-heading text-sm font-semibold tracking-wide text-slate-200">
            {t("aiProvider.addProvider")}
          </h3>

          <label className="block text-sm text-slate-300">
            {t("aiProvider.type")}
            <select
              value={form.providerType}
              onChange={(e) =>
                setForm({ ...form, providerType: e.target.value as ProviderType })
              }
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            >
              {PROVIDER_TYPES.map((type) => (
                <option key={type} value={type}>
                  {PROVIDER_TYPE_LABELS[type]}
                </option>
              ))}
            </select>
          </label>

          <label className="block text-sm text-slate-300">
            {t("aiProvider.providerName")}
            <input
              type="text"
              required
              value={form.displayName}
              onChange={(e) => setForm({ ...form, displayName: e.target.value })}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>

          <label className="block text-sm text-slate-300">
            {t("aiProvider.model")}
            <div className="mt-1 flex gap-2">
              <input
                type="text"
                required
                list={supportsModelDiscovery(form.providerType) ? MODEL_DATALIST_ID : undefined}
                placeholder={t("aiProvider.modelPlaceholder")}
                value={form.model}
                onChange={(e) => setForm({ ...form, model: e.target.value })}
                className="w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
              />
              {supportsModelDiscovery(form.providerType) && (
                <button
                  type="button"
                  onClick={handleDiscoverModels}
                  disabled={modelsLoading}
                  className="shrink-0 rounded border border-slate-600 px-2 py-1.5 text-xs whitespace-nowrap text-slate-300 hover:bg-slate-700 disabled:opacity-50"
                >
                  {modelsLoading ? t("aiProvider.discoveringModels") : t("aiProvider.discoverModels")}
                </button>
              )}
            </div>
            {/* Spec 0025, Abschnitt 2: `<datalist>` macht das Feld
             * durchsuchbar (Browser-Autocomplete über die entdeckten
             * Modelle), bleibt aber immer ein normales Freitextfeld — genau
             * der geforderte Fallback, falls die Discovery fehlschlägt oder
             * gar nicht erst versucht wird. */}
            <datalist id={MODEL_DATALIST_ID}>
              {models.map((model) => (
                <option key={model} value={model} />
              ))}
            </datalist>
            {modelsFailed && (
              <p className="mt-1 text-xs text-slate-500">{t("aiProvider.modelDiscoveryFailedHint")}</p>
            )}
          </label>

          {needsBaseUrl(form.providerType) && (
            <label className="block text-sm text-slate-300">
              {t("aiProvider.baseUrl")}
              <input
                type="text"
                required
                placeholder={t("aiProvider.baseUrlPlaceholder")}
                value={form.baseUrl ?? ""}
                onChange={(e) => setForm({ ...form, baseUrl: e.target.value })}
                className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
              />
            </label>
          )}

          <label className="block text-sm text-slate-300">
            {t("aiProvider.apiKey")}
            <input
              type="password"
              required
              value={form.apiKey}
              onChange={(e) => setForm({ ...form, apiKey: e.target.value })}
              className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
          </label>

          <label className="flex items-center gap-2 text-sm text-slate-300">
            <input
              type="checkbox"
              checked={form.supportsNativeToolCalling}
              onChange={(e) =>
                setForm({ ...form, supportsNativeToolCalling: e.target.checked })
              }
            />
            {t("aiProvider.nativeToolCalling")}
          </label>

          {/* Spec 0025, Abschnitt 3/4: Zusatz-Header und Attestierungs-URL
           * hinter einem "Erweitert"-Bereich, damit das Formular für den
           * Normalfall übersichtlich bleibt. */}
          <div className="border-t border-slate-700 pt-3">
            <button
              type="button"
              onClick={() => setShowAdvanced((prev) => !prev)}
              className="font-heading text-xs font-semibold tracking-wide text-slate-400 uppercase hover:text-slate-200"
            >
              {showAdvanced ? "▾ " : "▸ "}
              {t("aiProvider.advanced")}
            </button>

            {showAdvanced && (
              <div className="mt-3 space-y-3">
                <div>
                  <p className="text-sm text-slate-300">{t("aiProvider.extraHeadersLabel")}</p>
                  <p className="mt-0.5 text-xs text-slate-500">{t("aiProvider.extraHeadersHint")}</p>
                  <div className="mt-2 space-y-2">
                    {form.extraHeaders.map(([key, value], index) => (
                      <div key={index} className="flex gap-2">
                        <input
                          type="text"
                          placeholder={t("aiProvider.extraHeaderKeyPlaceholder")}
                          value={key}
                          onChange={(e) => updateExtraHeader(index, e.target.value, value)}
                          className="w-1/2 rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-sm text-slate-100"
                        />
                        <input
                          type="text"
                          placeholder={t("aiProvider.extraHeaderValuePlaceholder")}
                          value={value}
                          onChange={(e) => updateExtraHeader(index, key, e.target.value)}
                          className="w-1/2 rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-sm text-slate-100"
                        />
                        <button
                          type="button"
                          onClick={() => removeExtraHeader(index)}
                          aria-label={t("aiProvider.extraHeaderRemoveAria")}
                          className="shrink-0 px-1 text-slate-500 hover:text-slate-200"
                        >
                          ✕
                        </button>
                      </div>
                    ))}
                  </div>
                  <button
                    type="button"
                    onClick={addExtraHeader}
                    className="mt-2 rounded border border-slate-600 px-2 py-1 text-xs text-slate-300 hover:bg-slate-700"
                  >
                    {t("aiProvider.extraHeaderAdd")}
                  </button>
                </div>

                <label className="block text-sm text-slate-300">
                  {t("aiProvider.attestationUrlLabel")}
                  <input
                    type="text"
                    placeholder={t("aiProvider.attestationUrlPlaceholder")}
                    value={form.attestationUrl ?? ""}
                    onChange={(e) =>
                      setForm({ ...form, attestationUrl: e.target.value || null })
                    }
                    className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
                  />
                </label>
              </div>
            )}
          </div>

          <button
            type="submit"
            disabled={submitting}
            className="w-full rounded bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            {submitting ? t("aiProvider.adding") : t("aiProvider.add")}
          </button>
        </form>
      </div>
    </div>
  );
}
