import { type FormEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiKeyFormatWarning } from "../apiKeyFormat";
import {
  addAiProvider,
  commandErrorCode,
  commandErrorMessage,
  discoverModels,
  setActiveAiProvider,
  testAiProviderCredentials,
  updateAiProvider,
} from "../api";
import { Button } from "./ui";
import { DialogFrame } from "./ui/DialogFrame";
import { translateErrorCode } from "../errorCodes";
import { effectiveApiKey, OLLAMA_BASE_URL } from "../ollama";
import {
  type AiProviderConfigDto,
  type AiProviderConfigInput,
  type ProviderType,
  type TestAiProviderCredentialsResult,
  needsBaseUrl,
  supportsModelDiscovery,
  supportsWebResearch,
} from "../types";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";
import {
  type DiscoveryInputs,
  discoveryInputsOf,
  sameDiscoveryInputs,
  shouldAutoDiscover,
} from "../modelDiscoveryTrigger";
import { CARD_CLASS, FIELD_CLASS, LABEL_CLASS, LABEL_TEXT_CLASS } from "./aiProviderFormStyles";

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
    maxTokensOverride: null,
    // Spec 0105: Default an.
    webResearchEnabled: true,
  };
}

const MODEL_DATALIST_ID = "ai-provider-model-options";

/** Result of a successful save, handed to the parent so it can reload the
 * list and show an activation error on the page (Spec 0069, B6). */
export interface AddAiProviderResult {
  newId: string;
  /** The form carried an attestation URL: the parent fetches it. */
  fetchAttestation: boolean;
  /** Saving worked but making the provider active failed. */
  activationError: unknown;
}

function formFromProvider(provider: AiProviderConfigDto): AiProviderConfigInput {
  return {
    providerType: provider.providerType,
    displayName: provider.displayName,
    baseUrl: provider.baseUrl,
    model: provider.model,
    supportsNativeToolCalling: provider.supportsNativeToolCalling,
    // The stored key never reaches the frontend; empty = unchanged.
    apiKey: "",
    extraHeaders: provider.extraHeaders.map(([k, v]) => [k, v] as [string, string]),
    attestationUrl: provider.attestationUrl,
    maxTokensOverride: provider.maxTokensOverride,
    webResearchEnabled: provider.webResearchEnabled,
  };
}

interface AddAiProviderDialogProps {
  /** Issue #690: edit mode - the saved provider to edit. The type is shown
   * but fixed; an empty API key field keeps the stored credential. */
  provider?: AiProviderConfigDto;
  /** Edit mode: called after `update_ai_provider` succeeded. */
  onUpdated?: () => void;
  /** The provider list is loaded and none is active: the new provider is
   * activated (Spec 0069, B6). */
  willActivate: boolean;
  onClose: () => void;
  onAdded: (result: AddAiProviderResult) => void;
}

/** Issue #603 (Spec 0025): the add-provider form in a dialog stacked over the
 * Settings modal. All form-local state lives here, so closing the dialog
 * (Cancel, Escape) discards the entries - including the API key - and
 * reopening starts empty. */
export function AddAiProviderDialog({
  provider,
  onUpdated,
  willActivate,
  onClose,
  onAdded,
}: AddAiProviderDialogProps) {
  const { t } = useTranslation();
  const typeSelectRef = useRef<HTMLSelectElement>(null);
  const [error, setError] = useState<string | null>(null);
  const editing = provider !== undefined;
  const [form, setForm] = useState<AiProviderConfigInput>(() =>
    provider ? formFromProvider(provider) : emptyForm(),
  );
  // Issue #602: the name mirrors the model until the user edits the name.
  // The dialog is add-only and remounts per open, so this resets with it.
  const [nameEditedByUser, setNameEditedByUser] = useState(editing);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsFailed, setModelsFailed] = useState(false);
  // Spec 0069, Teil A5: bekannter Backend-Code des letzten Discovery-
  // Fehlschlags, für den übersetzten Grund unter dem bisherigen Hinweis —
  // `null` sowohl "kein Fehlschlag" als auch "Fehlschlag ohne bekannten
  // Code" (dann bleibt nur der bisherige generische Hinweis stehen).
  const [modelsFailedCode, setModelsFailedCode] = useState<string | null>(null);
  // Issue #326 (Spec 0025, Abschnitt 2): eine leere Liste ist kein gültiges
  // Ergebnis (Spec 0072 B-T4) — eigener neutraler Hinweis statt stillem
  // "Erfolg". `AI_AUTH_FAILED` bekommt einen eigenen, roten Zustand am
  // Key-Feld statt des generischen Discovery-Hinweises.
  const [modelsEmpty, setModelsEmpty] = useState(false);
  const [modelsAuthFailed, setModelsAuthFailed] = useState(false);
  // Issue #326: Eingaben (Typ, Base-URL, Key) der letzten automatischen
  // oder manuellen Discovery in diesem Formular — ein erneutes Verlassen
  // des Felds mit denselben Eingaben löst keine zweite Anfrage aus. Nur im
  // Speicher dieser Komponente, wie `form` selbst; nie geloggt.
  const lastDiscoveryInputsRef = useRef<DiscoveryInputs | null>(null);
  // Höchstens eine Discovery je Formular gleichzeitig; `formRef` liefert
  // beim Eintreffen der Antwort den aktuellen Stand, um das Ergebnis einer
  // inzwischen veralteten Anfrage zu verwerfen.
  const discoveryInFlightRef = useRef(false);
  const formRef = useRef(form);
  useEffect(() => {
    formRef.current = form;
  }, [form]);
  const [credentialTestRunning, setCredentialTestRunning] = useState(false);
  const [credentialTestResult, setCredentialTestResult] =
    useState<TestAiProviderCredentialsResult | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      if (provider) {
        // Issue #690: empty key = keep the stored credential.
        await updateAiProvider(provider.id, form);
        onUpdated?.();
        return;
      }
      // Spec 0069, Teil B5: Ollama mit leerem Key -> Platzhalter statt des
      // (dann leeren) `form.apiKey` - jeder andere Providertyp unverändert.
      const newId = await addAiProvider({ ...form, apiKey: effectiveApiKey(form) });
      // Spec 0069, Teil B6: Schlägt das Aktiv-Setzen fehl, ist der Provider
      // trotzdem gespeichert - der Dialog schließt, der Elternscreen zeigt
      // den Fehler (kein Löschen, kein doppeltes Anlegen).
      let activationError: unknown = null;
      if (willActivate) {
        try {
          await setActiveAiProvider(newId);
        } catch (err) {
          activationError = err;
        }
      }
      onAdded({ newId, fetchAttestation: Boolean(form.attestationUrl), activationError });
    } catch (err) {
      // The dialog stays open with all entered values.
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
      setSubmitting(false);
    }
  };

  /** Spec 0025, Abschnitt 2: läuft mit den aktuellen Formulardaten, auch
   * bevor der Provider gespeichert ist — schlägt der Aufruf fehl (nicht
   * jeder Anbieter unterstützt den Endpunkt zuverlässig), bleibt das
   * Modellfeld einfach ein normales Freitextfeld (kein `setError`, kein
   * blockierender Zustand).
   *
   * Issue #326: gemeinsamer Pfad für "Modelle laden" und den automatischen
   * Auslöser beim Verlassen des Key-/Base-URL-Felds. Höchstens eine
   * Anfrage je Formular gleichzeitig; ändern sich Typ, Base-URL oder Key,
   * während sie läuft, wird ihr Ergebnis verworfen. Der Key geht genau wie
   * bisher nur an `discover_models`, nirgends sonst hin. */
  // Edit mode: an empty field stays empty so the backend uses the stored key.
  const keyForRequest = (f: AiProviderConfigInput) => (editing ? f.apiKey : effectiveApiKey(f));

  const runModelDiscovery = async () => {
    if (discoveryInFlightRef.current) return;
    const requestForm = form;
    const requestInputs = discoveryInputsOf(requestForm);
    discoveryInFlightRef.current = true;
    lastDiscoveryInputsRef.current = requestInputs;
    setModelsLoading(true);
    setModelsFailed(false);
    setModelsFailedCode(null);
    setModelsEmpty(false);
    setModelsAuthFailed(false);
    const isCurrent = () =>
      sameDiscoveryInputs(requestInputs, discoveryInputsOf(formRef.current));
    try {
      const requestConfig = { ...requestForm, apiKey: keyForRequest(requestForm) };
      const discovered = provider
        ? await discoverModels(requestConfig, provider.id)
        : await discoverModels(requestConfig);
      if (!isCurrent()) return;
      setModels(discovered);
      setModelsEmpty(discovered.length === 0);
    } catch (err) {
      if (!isCurrent()) return;
      const code = commandErrorCode(err);
      setModels([]);
      if (code === "AI_AUTH_FAILED") {
        setModelsAuthFailed(true);
      } else {
        setModelsFailed(true);
        setModelsFailedCode(code);
      }
    } finally {
      discoveryInFlightRef.current = false;
      setModelsLoading(false);
    }
  };

  const handleDiscoverModels = () => {
    void runModelDiscovery();
  };

  /** Issue #326: das Verlassen des API-Key- oder Base-URL-Felds gilt als
   * Nutzeraktion, die die Modellsuche auslöst — bloßes Tippen nie. */
  const handleDiscoveryFieldBlur = () => {
    if (discoveryInFlightRef.current) return;
    if (!shouldAutoDiscover(form, lastDiscoveryInputsRef.current)) return;
    void runModelDiscovery();
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
      const testConfig = { ...form, apiKey: keyForRequest(form) };
      const result = provider
        ? await testAiProviderCredentials(testConfig, provider.id)
        : await testAiProviderCredentials(testConfig);
      setCredentialTestResult(result);
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setCredentialTestRunning(false);
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

  const apiKeyWarning = apiKeyFormatWarning(form.providerType, form.baseUrl, form.apiKey);

  return (
    <DialogFrame
      layer="app"
      size="lg"
      title={editing ? t("aiProvider.editProvider") : t("aiProvider.addProvider")}
      onClose={onClose}
      onSubmit={handleSubmit}
      initialFocusRef={typeSelectRef}
      actions={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button
            type="submit"
            variant="primary"
            disabled={submitting || (form.maxTokensOverride !== null && form.maxTokensOverride <= 0)}
          >
            {editing
              ? submitting
                ? t("common.saving")
                : t("common.save")
              : submitting
                ? t("aiProvider.adding")
                : t("aiProvider.add")}
          </Button>
        </>
      }
    >
      <div className="space-y-4 p-0.5">
        {error && (
          <p role="alert" className="rounded bg-red-950 px-3 py-2 text-sm text-red-300">
            {error}
          </p>
        )}
        <section className={`${CARD_CLASS} space-y-3`}>
          <label className={LABEL_CLASS}>
            <span className={LABEL_TEXT_CLASS}>{t("aiProvider.type")}</span>
            <select
              ref={typeSelectRef}
              value={form.providerType}
              disabled={editing}
              onChange={(e) => {
                const nextType = e.target.value as ProviderType;
                setForm({
                  ...form,
                  providerType: nextType,
                  // Spec 0069, Teil B5: Base-URL bei Wechsel zu Ollama
                  // vorbelegen, wenn sie noch leer ist — sichtbar,
                  // änderbar, nie ein bereits eingegebener Wert
                  // überschrieben.
                  baseUrl:
                    nextType === "ollama" && !form.baseUrl?.trim()
                      ? OLLAMA_BASE_URL
                      : form.baseUrl,
                });
                setCredentialTestResult(null);
                setModelsAuthFailed(false);
              }}
              className={`mt-1 w-full ${FIELD_CLASS}`}
            >
              {PROVIDER_TYPES.map((type) => (
                <option key={type} value={type}>
                  {t(`aiProvider.providerTypes.${type}`)}
                </option>
              ))}
            </select>
          </label>

          <label className={LABEL_CLASS}>
            <span className={LABEL_TEXT_CLASS}>{t("aiProvider.providerName")}</span>
            <input
              {...TECHNICAL_INPUT_PROPS}
              type="text"
              required
              value={form.displayName}
              onChange={(e) => {
                setNameEditedByUser(true);
                setForm({ ...form, displayName: e.target.value });
              }}
              className={`mt-1 w-full ${FIELD_CLASS}`}
            />
          </label>

          <label className={LABEL_CLASS}>
            <span className={LABEL_TEXT_CLASS}>{t("aiProvider.model")}</span>
            <div className="mt-1 flex gap-2">
              <input
                {...TECHNICAL_INPUT_PROPS}
                type="text"
                required
                list={supportsModelDiscovery(form.providerType) ? MODEL_DATALIST_ID : undefined}
                placeholder={t("aiProvider.modelPlaceholder")}
                value={form.model}
                onChange={(e) =>
                  setForm({
                    ...form,
                    model: e.target.value,
                    displayName: nameEditedByUser ? form.displayName : e.target.value,
                  })
                }
                className={`w-full ${FIELD_CLASS}`}
              />
              {supportsModelDiscovery(form.providerType) && (
                <Button size="sm"
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
                  
                >
                  {modelsLoading ? t("aiProvider.discoveringModels") : t("aiProvider.discoverModels")}
                </Button>
              )}
            </div>
            {/* Spec 0025, Abschnitt 2: `<datalist>` macht das Feld
             * durchsuchbar (Browser-Autocomplete über die entdeckten
             * Modelle), bleibt aber immer ein normales Freitextfeld — genau
             * der geforderte Fallback, falls die Discovery fehlschlägt oder
             * gar nicht erst versucht wird. */}
            <datalist id={MODEL_DATALIST_ID}>
              {models.map((model) => (
                <option key={model} value={model} aria-label={model} />
              ))}
            </datalist>
            {modelsEmpty && (
              <p className="mt-1 text-xs text-slate-400">{t("aiProvider.modelDiscoveryEmptyHint")}</p>
            )}
            {modelsFailed && (
              <>
                <p className="mt-1 text-xs text-slate-400">{t("aiProvider.modelDiscoveryFailedHint")}</p>
                {/* Spec 0069, Teil A5: zusätzlich der übersetzte Grund,
                 * wenn der Fehler einen bekannten Code trägt (z. B.
                 * AI_MODEL_NOT_FOUND, AI_LOCAL_PROVIDER_UNREACHABLE) —
                 * unter dem bisherigen, unveränderten Hinweis, nicht
                 * anstelle davon. `translateErrorCode` fällt bei
                 * unbekanntem Code auf den leeren String zurück, den wir
                 * hier explizit unterdrücken statt eine leere Box zu
                 * zeigen. */}
                {modelsFailedCode && translateErrorCode(t, modelsFailedCode, "") && (
                  <p className="mt-0.5 text-xs text-slate-400">
                    {translateErrorCode(t, modelsFailedCode, "")}
                  </p>
                )}
              </>
            )}
          </label>

          {needsBaseUrl(form.providerType) && (
            <label className={LABEL_CLASS}>
              <span className={LABEL_TEXT_CLASS}>{t("aiProvider.baseUrl")}</span>
              <input
                {...TECHNICAL_INPUT_PROPS}
                type="text"
                required
                placeholder={t("aiProvider.baseUrlPlaceholder")}
                value={form.baseUrl ?? ""}
                onChange={(e) => {
                  setForm({ ...form, baseUrl: e.target.value });
                  setModelsAuthFailed(false);
                }}
                onBlur={handleDiscoveryFieldBlur}
                className={`mt-1 w-full ${FIELD_CLASS}`}
              />
            </label>
          )}

          <label className={LABEL_CLASS}>
            <span className={LABEL_TEXT_CLASS}>
              {t("aiProvider.apiKey")}
              {/* Spec 0069, Teil B5: Ollama braucht keinen Key. */}
              {form.providerType === "ollama" && (
                <span className="ml-1 font-normal text-slate-400">
                  {t("aiProvider.apiKeyNotNeededForOllama")}
                </span>
              )}
            </span>
            <input
              {...TECHNICAL_INPUT_PROPS}
              type="password"
              required={!editing && form.providerType !== "ollama"}
              placeholder={editing ? t("aiProvider.apiKeyUnchangedPlaceholder") : undefined}
              value={form.apiKey}
              onChange={(e) => {
                setForm({ ...form, apiKey: e.target.value });
                // Spec 0050, Teil 3: ein Testergebnis bezieht sich auf den
                // Stand zum Testzeitpunkt — ändert sich der Key danach,
                // wäre ein weiterhin angezeigtes "gültig" irreführend.
                setCredentialTestResult(null);
                // Issue #326: "Zugangsdaten abgelehnt" gilt nur für den
                // Key, mit dem die Modellsuche lief.
                setModelsAuthFailed(false);
              }}
              onBlur={handleDiscoveryFieldBlur}
              className={`mt-1 w-full ${FIELD_CLASS}`}
            />
          </label>
          {/* Issue #326: eigener Zustand für `AI_AUTH_FAILED` aus der
           * Modellsuche, im selben roten Stil wie das Ergebnis von
           * "Zugangsdaten testen" — nicht der generische Discovery-Hinweis. */}
          {modelsAuthFailed && (
            <p
              role="alert"
              className="rounded border border-red-800 bg-red-950/40 px-2.5 py-1.5 text-xs text-red-400"
            >
              {t("aiProvider.modelDiscoveryAuthFailed")}
            </p>
          )}
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
                providerLabel: t(`aiProvider.providerTypes.${form.providerType}`),
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
            <Button size="sm"
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
                // Spec 0069, Teil B5: bei Ollama ist ein leerer Key kein
                // Grund zu sperren — "Zugangsdaten testen" ist ohne Key
                // freigegeben.
                (!editing && form.providerType !== "ollama" && !form.apiKey.trim()) ||
                (needsBaseUrl(form.providerType) && !form.baseUrl?.trim())
              }
              
            >
              {credentialTestRunning ? t("aiProvider.testingCredentials") : t("aiProvider.testCredentials")}
            </Button>
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
                  // Spec 0069, Teil A5: bekannter Code → übersetzte
                  // Meldung statt des rohen Backend-Texts. Sonderfall
                  // AI_RATE_LIMITED: eigener Text (die Chat-Meldung sagt
                  // "Nachricht erneut senden" — passt im Testen-Kontext
                  // nicht). Ohne Code: bisheriges Verhalten.
                  (credentialTestResult.code === "AI_RATE_LIMITED"
                    ? t("aiProvider.testResultRateLimited")
                    : translateErrorCode(
                        t,
                        credentialTestResult.code,
                        t("aiProvider.testResultUnreachable", {
                          message: credentialTestResult.message,
                        }),
                      ))}
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

          {supportsWebResearch(form.providerType, form.baseUrl) && (
            <div>
              <label className="flex items-center gap-2 text-sm text-slate-300">
                <input
                  type="checkbox"
                  checked={form.webResearchEnabled}
                  onChange={(e) => setForm({ ...form, webResearchEnabled: e.target.checked })}
                />
                {t("aiProvider.webResearch")}
              </label>
              <p className="mt-1 text-xs text-slate-400">{t("aiProvider.webResearchHint")}</p>
            </div>
          )}

          {/* Spec 0025, Abschnitt 3/4: Zusatz-Header und Attestierungs-URL
           * hinter einem "Erweitert"-Bereich, damit das Formular für den
           * Normalfall übersichtlich bleibt. Spec 0056, Teil 3: der
           * aufgeklappte Bereich bekommt jetzt einen eigenen, leicht
           * abgesetzten Innenrahmen (dunkler als die umgebende Karte) —
           * macht sichtbar, dass das eine verschachtelte, optionale
           * Untergruppe ist, nicht gleichrangig mit den Kernfeldern
           * darüber. */}
          <div className="border-t border-slate-700/60 pt-3">
            <Button
              variant="ghost"
              size="sm"
              aria-expanded={showAdvanced}
              onClick={() => setShowAdvanced((prev) => !prev)}
              className="font-heading tracking-wide uppercase"
            >
              <span aria-hidden="true">{showAdvanced ? "▾" : "▸"}</span>
              {t("aiProvider.advanced")}
            </Button>

            {showAdvanced && (
              <div className="mt-3 space-y-3 rounded-md border border-slate-800 bg-slate-950/30 p-3">
                <div>
                  <p className="text-sm text-slate-300">{t("aiProvider.extraHeadersLabel")}</p>
                  <p className="mt-0.5 text-xs text-slate-400">{t("aiProvider.extraHeadersHint")}</p>
                  <div className="mt-2 space-y-2">
                    {form.extraHeaders.map(([key, value], index) => (
                      <div key={index} className="flex gap-2">
                        <input
                          {...TECHNICAL_INPUT_PROPS}
                          type="text"
                          aria-label={t("aiProvider.extraHeaderKeyPlaceholder")}
                          placeholder={t("aiProvider.extraHeaderKeyPlaceholder")}
                          value={key}
                          onChange={(e) => updateExtraHeader(index, e.target.value, value)}
                          className={`w-1/2 text-sm ${FIELD_CLASS}`}
                        />
                        <input
                          {...TECHNICAL_INPUT_PROPS}
                          type="text"
                          aria-label={t("aiProvider.extraHeaderValuePlaceholder")}
                          placeholder={t("aiProvider.extraHeaderValuePlaceholder")}
                          value={value}
                          onChange={(e) => updateExtraHeader(index, key, e.target.value)}
                          className={`w-1/2 text-sm ${FIELD_CLASS}`}
                        />
                        <Button
                          size="sm"
                          onClick={() => removeExtraHeader(index)}
                          aria-label={t("aiProvider.extraHeaderRemoveAria")}
                          className="shrink-0"
                        >
                          <span aria-hidden="true">✕</span>
                        </Button>
                      </div>
                    ))}
                  </div>
                  <Button size="sm" onClick={addExtraHeader} className="mt-2">
                    {t("aiProvider.extraHeaderAdd")}
                  </Button>
                </div>

                <label className={LABEL_CLASS}>
                  <span className={LABEL_TEXT_CLASS}>{t("aiProvider.attestationUrlLabel")}</span>
                  <input
                    {...TECHNICAL_INPUT_PROPS}
                    type="text"
                    placeholder={t("aiProvider.attestationUrlPlaceholder")}
                    value={form.attestationUrl ?? ""}
                    onChange={(e) =>
                      setForm({ ...form, attestationUrl: e.target.value || null })
                    }
                    className={`mt-1 w-full ${FIELD_CLASS}`}
                  />
                </label>

                {/* Spec 0065, Teil 4: optionaler max_tokens-Override, Default
                 * "Automatisch" (leer). Serverseitig nochmals validiert
                 * (`AiProviderConfigInput::validate_max_tokens_override`) —
                 * diese Prüfung ist nur die schnelle, clientseitige
                 * Rückmeldung. */}
                <label className={LABEL_CLASS}>
                  <span className={LABEL_TEXT_CLASS}>{t("aiProvider.maxTokensOverrideLabel")}</span>
                  <input
                    type="number"
                    min={1}
                    placeholder={t("aiProvider.maxTokensOverridePlaceholder")}
                    value={form.maxTokensOverride ?? ""}
                    onChange={(e) => {
                      const raw = e.target.value;
                      setForm({
                        ...form,
                        maxTokensOverride: raw === "" ? null : Number(raw),
                      });
                    }}
                    className={`mt-1 w-full ${FIELD_CLASS}`}
                  />
                  <p className="mt-0.5 text-xs text-slate-400">
                    {t("aiProvider.maxTokensOverrideHint")}
                  </p>
                  {form.maxTokensOverride !== null && form.maxTokensOverride <= 0 && (
                    <p className="mt-0.5 text-xs text-red-400">
                      {t("aiProvider.maxTokensOverrideMustBePositive")}
                    </p>
                  )}
                </label>
              </div>
            )}
          </div>
        </section>

        {/* Spec 0069, Teil B6: Hinweis vor dem Speichern, nur solange kein
            Provider aktiv ist. */}
        {!editing && willActivate && (
          <p className="text-xs text-slate-400">{t("aiProvider.addHintWillActivate")}</p>
        )}
      </div>
    </DialogFrame>
  );
}
