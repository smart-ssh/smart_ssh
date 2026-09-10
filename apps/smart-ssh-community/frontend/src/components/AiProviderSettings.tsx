import { type FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiKeyFormatWarning } from "../apiKeyFormat";
import {
  addAiProvider,
  commandErrorMessage,
  deleteAiProvider,
  discoverModels,
  fetchAttestationInfo,
  listAiProviders,
  setActiveAiProvider,
  testAiProviderCredentials,
} from "../api";
import { loadRiskClassifierSettings, saveRiskClassifierSettings } from "../riskSettings";
import {
  type AiProviderConfigDto,
  type AiProviderConfigInput,
  type ProviderType,
  type TestAiProviderCredentialsResult,
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

/** Spec 0056, Teil 3: gemeinsame Design-Tokens für dieses Formular statt
 * pro Feld wiederholter Ad-hoc-Klassenketten — alle vier Farbpaletten
 * (`slate`/`indigo`/`emerald`/`red`) sind bereits projektweite Tokens
 * (`index.css`s `@theme`), hier nur konsistent auf Formularfelder/
 * Sekundäraktionen angewandt. `bg-slate-950` für Felder INNERHALB einer
 * `bg-slate-900/40`-Karte erzeugt die vom Reviewer/Stefan gewünschte
 * Kontraststufung (Modal-Grund `slate-800` → Karte `slate-900/40` → Feld
 * `slate-950`), `focus:ring-indigo-500` ist der bislang fehlende sichtbare
 * Fokus-Zustand. */
const CARD_CLASS = "rounded-lg border border-slate-700 bg-slate-900/40 p-4";
const LABEL_CLASS = "block text-sm font-medium text-slate-200";
const FIELD_CLASS =
  "mt-1 w-full rounded border border-slate-600 bg-slate-950 px-2.5 py-1.5 text-slate-100 outline-none transition-colors focus:border-indigo-500 focus:ring-2 focus:ring-indigo-500/40";
const SECONDARY_BUTTON_CLASS =
  "shrink-0 rounded border border-slate-600 bg-slate-800 px-2.5 py-1.5 text-xs font-medium whitespace-nowrap text-slate-200 transition-colors hover:border-slate-500 hover:bg-slate-700 focus:outline-none focus:ring-2 focus:ring-indigo-500/40 disabled:cursor-not-allowed disabled:opacity-50";

interface AiProviderSettingsProps {
  /** Löst neu laden von `list_ai_providers` im Elternscreen aus (z. B. für
   * den "kein Provider konfiguriert"-Hinweis), sobald sich hier etwas
   * ändert — kein globaler State-Store in Teil 1, dafür reicht ein
   * simpler Callback. */
  onProvidersChanged: () => void;
}

/** Spec 0050, Teil 1: reine Kategorie-Inhaltskomponente für "KI-Provider"
 * in der zweispaltigen Settings-Struktur (`SettingsScreen.tsx`) — trägt
 * seit diesem Umbau weder den Modal-Rahmen noch den Sprachumschalter/
 * Log-Verzeichnis-Button/registrierte Sektionen mehr (jetzt eigene
 * Kategorien, s. `LanguageSettings.tsx`/`DiagnosticsSettings.tsx` bzw.
 * `SettingsScreen.tsx`s generisches Rendern registrierter Sektionen). */
export function AiProviderSettings({ onProvidersChanged }: AiProviderSettingsProps) {
  const { t } = useTranslation();
  const [providers, setProviders] = useState<AiProviderConfigDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<AiProviderConfigInput>(emptyForm());
  const [submitting, setSubmitting] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsFailed, setModelsFailed] = useState(false);
  const [credentialTestRunning, setCredentialTestRunning] = useState(false);
  const [credentialTestResult, setCredentialTestResult] =
    useState<TestAiProviderCredentialsResult | null>(null);
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
      setCredentialTestResult(null);
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

  /** Spec 0050, Teil 3: testet die gerade eingegebenen, noch nicht
   * gespeicherten Formulardaten mit einem echten Mini-Request — analog zu
   * `handleDiscoverModels` oben, aber mit einem klar dreiwertigen Ergebnis
   * statt "Erfolg oder Fallback aufs Freitextfeld". Ein `catch` hier ist
   * ein echter Bedienfehler (z. B. kein API-Key gesetzt), nicht einer der
   * drei regulären Testausgänge — die kommen als normaler Rückgabewert,
   * kein Wurf. */
  const handleTestCredentials = async () => {
    setCredentialTestRunning(true);
    setCredentialTestResult(null);
    try {
      const result = await testAiProviderCredentials(form);
      setCredentialTestResult(result);
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setCredentialTestRunning(false);
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

  const apiKeyWarning = apiKeyFormatWarning(form.providerType, form.baseUrl, form.apiKey);

  return (
    <div>
      {error && (
        <p className="mb-4 rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>
      )}

      {/* Spec 0056, Teil 3: eigene Karte statt freistehender Liste — macht
       * sichtbar, dass dies ein abgeschlossener Bereich ("was ist bereits
       * konfiguriert") ist, getrennt vom Formular darunter ("neuen Provider
       * anlegen"). Die Liste selbst behält ihre bisherige `divide-y`-
       * Struktur, bekommt aber einen dunkleren Innenhintergrund
       * (`slate-950/40`) als die Karte (`slate-900/40`), damit sich Karte
       * und Listeninhalt trotz gleicher Rahmenfarbe voneinander abheben. */}
      <section className={`mb-6 ${CARD_CLASS}`}>
        <h3 className="font-heading mb-3 text-sm font-semibold tracking-wide text-slate-200">
          {t("aiProvider.configuredProvidersTitle")}
        </h3>
        <ul className="divide-y divide-slate-700 rounded-md border border-slate-700 bg-slate-950/40">
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
                      className="rounded border border-slate-600 bg-slate-700 px-2 py-1 text-xs text-slate-100 transition-colors hover:border-slate-500 hover:bg-slate-600 focus:outline-none focus:ring-2 focus:ring-indigo-500/40"
                    >
                      {t("aiProvider.setActive")}
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => handleDelete(provider.id)}
                    className="rounded border border-red-800 bg-red-900 px-2 py-1 text-xs text-red-200 transition-colors hover:bg-red-800 focus:outline-none focus:ring-2 focus:ring-red-500/40"
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
                    className={SECONDARY_BUTTON_CLASS}
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
      </section>

      {/* Spec 0026, Abschnitt 3, Punkt 1: eigener Abschnitt für die
       * optionale KI-Zweitmeinung zur Daten-Risiko-Achse — standardmäßig
       * deaktiviert (Opt-in), separat wählbarer Provider, Hinweis auf ein
       * empfohlenes lokales Modell. Spec 0056, Teil 3: eigene Karte statt
       * einer bloßen oberen Trennlinie — bislang lief dieser Bereich
       * optisch nahtlos in die Provider-Liste über. */}
      <section className={`mb-6 ${CARD_CLASS}`}>
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
          <label className={LABEL_CLASS}>
            {t("aiProvider.riskClassifierProvider")}
            <select
              value={riskClassifierProviderId ?? ""}
              onChange={(e) => handleRiskClassifierChange(true, e.target.value || null)}
              disabled={riskSettingsSaving}
              className={FIELD_CLASS}
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
      </section>

      <form onSubmit={handleSubmit} className="space-y-4">
        {/* Spec 0056, Teil 3: Kernfelder (Typ/Name/Modell/Base-URL/Key) als
         * eine zusammengehörige Karte — vorher liefen sie ohne jede
         * Rahmung direkt unter der Risiko-Zweitmeinung-Sektion weiter, kaum
         * von ihr zu unterscheiden. */}
        <section className={`${CARD_CLASS} space-y-3`}>
          <h3 className="font-heading text-sm font-semibold tracking-wide text-slate-200">
            {t("aiProvider.addProvider")}
          </h3>

          <label className={LABEL_CLASS}>
            {t("aiProvider.type")}
            <select
              value={form.providerType}
              onChange={(e) => {
                setForm({ ...form, providerType: e.target.value as ProviderType });
                setCredentialTestResult(null);
              }}
              className={FIELD_CLASS}
            >
              {PROVIDER_TYPES.map((type) => (
                <option key={type} value={type}>
                  {PROVIDER_TYPE_LABELS[type]}
                </option>
              ))}
            </select>
          </label>

          <label className={LABEL_CLASS}>
            {t("aiProvider.providerName")}
            <input
              type="text"
              required
              value={form.displayName}
              onChange={(e) => setForm({ ...form, displayName: e.target.value })}
              className={FIELD_CLASS}
            />
          </label>

          <label className={LABEL_CLASS}>
            {t("aiProvider.model")}
            <div className="mt-1 flex gap-2">
              <input
                type="text"
                required
                list={supportsModelDiscovery(form.providerType) ? MODEL_DATALIST_ID : undefined}
                placeholder={t("aiProvider.modelPlaceholder")}
                value={form.model}
                onChange={(e) => setForm({ ...form, model: e.target.value })}
                className={`${FIELD_CLASS} mt-0 w-full`}
              />
              {supportsModelDiscovery(form.providerType) && (
                <button
                  type="button"
                  onClick={handleDiscoverModels}
                  // Unabhängiger Review-Pass (Spec 0024/0025): dieser Button
                  // ist `type="button"`, das `required`-Attribut des
                  // Base-URL-Felds greift also nicht, solange der Nutzer
                  // nicht submitted. Ohne diese Sperre könnte ein Klick vor
                  // dem Ausfüllen der Base-URL den API-Key an den
                  // Backend-Fallback (OpenAI) statt an den gewählten
                  // Endpunkt schicken (serverseitig zusätzlich in
                  // `discover_models` abgefangen).
                  disabled={
                    modelsLoading ||
                    (needsBaseUrl(form.providerType) && !form.baseUrl?.trim())
                  }
                  className={SECONDARY_BUTTON_CLASS}
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
            <label className={LABEL_CLASS}>
              {t("aiProvider.baseUrl")}
              <input
                type="text"
                required
                placeholder={t("aiProvider.baseUrlPlaceholder")}
                value={form.baseUrl ?? ""}
                onChange={(e) => setForm({ ...form, baseUrl: e.target.value })}
                className={FIELD_CLASS}
              />
            </label>
          )}

          <label className={LABEL_CLASS}>
            {t("aiProvider.apiKey")}
            <input
              type="password"
              required
              value={form.apiKey}
              onChange={(e) => {
                setForm({ ...form, apiKey: e.target.value });
                // Spec 0050, Teil 3: ein Testergebnis bezieht sich auf den
                // Stand zum Testzeitpunkt — ändert sich der Key danach,
                // wäre ein weiterhin angezeigtes "gültig" irreführend.
                setCredentialTestResult(null);
              }}
              className={FIELD_CLASS}
            />
          </label>
          {/* Spec 0050, Teil 2 (jetzt auch Spec 0056, Teil 1): reiner
           * Offline-Hinweis, kein Blockieren — `apiKeyFormatWarning`
           * liefert `null`, solange das Feld leer ist, der Provider kein
           * vorhersagbares Format hat, oder das Präfix passt. Der
           * Submit-Handler prüft dieses Ergebnis nicht; Speichern bleibt in
           * jedem Fall möglich. Spec 0056, Teil 3: als eigene, klar
           * umrandete Hinweisbox statt bloßem Fließtext — bislang ging der
           * Hinweis optisch kaum vom Rest des Formulars unterscheidbar
           * unter. */}
          {apiKeyWarning && (
            <p className="rounded border border-amber-800 bg-amber-950/40 px-2.5 py-1.5 text-xs text-amber-300">
              {t("aiProvider.apiKeyFormatHint", {
                providerLabel: PROVIDER_TYPE_LABELS[form.providerType],
                expectedPrefix: apiKeyWarning.expectedPrefix,
              })}
            </p>
          )}

          {/* Spec 0050, Teil 3 (jetzt auch Spec 0056, Teil 2): "Testen"-
           * Button — analog zum "Verbindung testen" bei Servern (Spec
           * 0008), nutzt die gerade eingegebenen, noch nicht gespeicherten
           * Formulardaten. Bereits mit der von Spec 0056 verlangten
           * Drei-Ergebnis-Unterscheidung (gültig/Auth fehlgeschlagen/nicht
           * erreichbar) und demselben Base-URL-Guard wie "Modelle laden" —
           * hier nur visuell (eigene Ergebnis-Box) in die neue Struktur
           * eingebettet, keine funktionale Änderung. */}
          <div>
            <button
              type="button"
              onClick={handleTestCredentials}
              // Spec-Reviewer-Fund (Spec 0050, Review dieses Schritts):
              // dieselbe Sperre wie beim "Modelle laden"-Button oben — ohne
              // sie würde der eingegebene API-Key bei generic_openai_
              // compatible/ollama ohne ausgefüllte Base-URL an den
              // Backend-Fallback (OpenAI) statt an den gewählten Endpunkt
              // gehen (serverseitig zusätzlich in
              // `test_ai_provider_credentials` abgefangen).
              disabled={
                credentialTestRunning ||
                !form.apiKey.trim() ||
                (needsBaseUrl(form.providerType) && !form.baseUrl?.trim())
              }
              className={SECONDARY_BUTTON_CLASS}
            >
              {credentialTestRunning ? t("aiProvider.testingCredentials") : t("aiProvider.testCredentials")}
            </button>
            {credentialTestResult && (
              <p
                className={`mt-1.5 rounded border px-2.5 py-1.5 text-xs ${
                  credentialTestResult.kind === "valid"
                    ? "border-emerald-800 bg-emerald-950/40 text-emerald-400"
                    : "border-red-800 bg-red-950/40 text-red-400"
                }`}
              >
                {credentialTestResult.kind === "valid" && t("aiProvider.testResultValid")}
                {credentialTestResult.kind === "authenticationFailed" &&
                  t("aiProvider.testResultAuthFailed")}
                {credentialTestResult.kind === "unreachable" &&
                  t("aiProvider.testResultUnreachable", { message: credentialTestResult.message })}
              </p>
            )}
          </div>

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
           * Normalfall übersichtlich bleibt. Spec 0056, Teil 3: der
           * aufgeklappte Bereich bekommt jetzt einen eigenen, leicht
           * abgesetzten Innenrahmen (dunkler als die umgebende Karte) —
           * macht sichtbar, dass das eine verschachtelte, optionale
           * Untergruppe ist, nicht gleichrangig mit den Kernfeldern
           * darüber. */}
          <div className="border-t border-slate-700/60 pt-3">
            <button
              type="button"
              onClick={() => setShowAdvanced((prev) => !prev)}
              className="font-heading text-xs font-semibold tracking-wide text-slate-400 uppercase hover:text-slate-200"
            >
              {showAdvanced ? "▾ " : "▸ "}
              {t("aiProvider.advanced")}
            </button>

            {showAdvanced && (
              <div className="mt-3 space-y-3 rounded-md border border-slate-800 bg-slate-950/30 p-3">
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
                          className={`${FIELD_CLASS} mt-0 w-1/2 text-sm`}
                        />
                        <input
                          type="text"
                          placeholder={t("aiProvider.extraHeaderValuePlaceholder")}
                          value={value}
                          onChange={(e) => updateExtraHeader(index, key, e.target.value)}
                          className={`${FIELD_CLASS} mt-0 w-1/2 text-sm`}
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
                  <button type="button" onClick={addExtraHeader} className={`mt-2 ${SECONDARY_BUTTON_CLASS}`}>
                    {t("aiProvider.extraHeaderAdd")}
                  </button>
                </div>

                <label className={LABEL_CLASS}>
                  {t("aiProvider.attestationUrlLabel")}
                  <input
                    type="text"
                    placeholder={t("aiProvider.attestationUrlPlaceholder")}
                    value={form.attestationUrl ?? ""}
                    onChange={(e) =>
                      setForm({ ...form, attestationUrl: e.target.value || null })
                    }
                    className={FIELD_CLASS}
                  />
                </label>
              </div>
            )}
          </div>
        </section>

        <button
          type="submit"
          disabled={submitting}
          className="w-full rounded bg-indigo-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-500 focus:outline-none focus:ring-2 focus:ring-indigo-500/60 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {submitting ? t("aiProvider.adding") : t("aiProvider.add")}
        </button>
      </form>
    </div>
  );
}
