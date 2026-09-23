import { type FormEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiKeyFormatWarning } from "../apiKeyFormat";
import {
  addAiProvider,
  commandErrorCode,
  commandErrorMessage,
  deleteAiProvider,
  discoverModels,
  fetchAttestationInfo,
  listAiProviders,
  setActiveAiProvider,
  testAiProviderCredentials,
} from "../api";
import { translateErrorCode } from "../errorCodes";
import { effectiveApiKey, OLLAMA_BASE_URL, OLLAMA_PLACEHOLDER_API_KEY } from "../ollama";
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
    maxTokensOverride: null,
  };
}

const MODEL_DATALIST_ID = "ai-provider-model-options";

/** Spec 0069, Teil B3: die fünf unterscheidbaren Anzeigezustände der
 * Ollama-Erkennungskarte. Kein eigener `idle`-artiger Zustand nötig —
 * `null` (statt eines Werts dieses Typs) heißt "keine Karte", s.
 * `AiProviderSettings`s `ollamaProbe`-State. */
type OllamaProbeState =
  | { status: "loading" }
  | { status: "found"; models: string[] }
  | { status: "empty" }
  | { status: "unreachable" }
  | { status: "otherError" };

/** Spec 0056, Teil 3: gemeinsame Design-Tokens für dieses Formular statt
 * pro Feld wiederholter Ad-hoc-Klassenketten — alle vier Farbpaletten
 * (`slate`/`indigo`/`emerald`/`red`) sind bereits projektweite Tokens
 * (`index.css`s `@theme`), hier nur konsistent auf Formularfelder/
 * Sekundäraktionen angewandt. `bg-slate-950` für Felder INNERHALB einer
 * `bg-slate-900/40`-Karte erzeugt die vom Reviewer/Stefan gewünschte
 * Kontraststufung (Modal-Grund `slate-800` → Karte `slate-900/40` → Feld
 * `slate-950`), `focus:ring-indigo-500` ist der bislang fehlende sichtbare
 * Fokus-Zustand.
 *
 * Spec-Reviewer-Fund (Spec 0056, Review dieses Schritts): `FIELD_CLASS`
 * enthielt ursprünglich auch Layout-Klassen (`mt-1 w-full`) — an drei
 * Stellen per `` `${FIELD_CLASS} mt-0 w-1/2` `` überschrieben. Tailwind
 * löst widersprüchliche Utility-Klassen über die Reihenfolge im
 * generierten Stylesheet, NICHT über die Reihenfolge im `className`-String
 * — die Overrides griffen deshalb nie (`mt-1`/`w-full` gewannen immer),
 * mit sichtbarem Versatz beim Modell-Feld/den Zusatz-Header-Feldern. Layout
 * (Abstand/Breite) ist jetzt bewusst NICHT Teil von `FIELD_CLASS`, sondern
 * wird an jeder Verwendungsstelle explizit gesetzt — ein echter Konflikt
 * ist damit strukturell ausgeschlossen. Ebenso `LABEL_CLASS`: `font-medium`
 * lag vorher auf dem `<label>` selbst und vererbte sich dadurch auf die
 * darin verschachtelten `<input>`/`<select>` (Tailwind-Preflight setzt dort
 * `font: inherit`) — Nutzereingaben erschienen halbfett. Der Text-Stil
 * liegt jetzt auf einem separaten `<span>` um den Label-Text, nicht mehr
 * auf dem Label-Element. */
const CARD_CLASS = "rounded-lg border border-slate-700 bg-slate-900/40 p-4";
const LABEL_CLASS = "block text-sm";
const LABEL_TEXT_CLASS = "font-medium text-slate-200";
const FIELD_CLASS =
  "rounded border border-slate-600 bg-slate-950 px-2.5 py-1.5 text-slate-100 outline-none transition-colors focus:border-indigo-500 focus:ring-2 focus:ring-indigo-500/40";
// Spec-Reviewer-Fund: `focus:` (statt `focus-visible:`) hielt den Ring auf
// Buttons auch nach einem reinen Mausklick sichtbar stehen, statt nur bei
// Tastaturfokus — für Eingabefelder ist `focus:` üblich (Klick = Fokus =
// aktive Eingabe), für Buttons wechselt diese Klasse deshalb auf
// `focus-visible:`.
const SECONDARY_BUTTON_CLASS =
  "shrink-0 rounded border border-slate-600 bg-slate-800 px-2.5 py-1.5 text-xs font-medium whitespace-nowrap text-slate-200 transition-colors hover:border-slate-500 hover:bg-slate-700 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/40 disabled:cursor-not-allowed disabled:opacity-50";
// Spec-Reviewer-Fund: "Aktiv setzen"/"Löschen" trugen eigene, von
// `SECONDARY_BUTTON_CLASS` abweichende Ad-hoc-Klassenketten (kleinere
// Innenabstände) — für eine konsistente Aktionshierarchie jetzt dieselbe
// Größe/denselben Rahmen-Stil wie jeder andere sekundäre Button, nur mit
// den etablierten semantischen Farben (Spec 0009: `red` = destruktiv).
const DANGER_BUTTON_CLASS =
  "shrink-0 rounded border border-red-800 bg-red-900 px-2.5 py-1.5 text-xs font-medium whitespace-nowrap text-red-200 transition-colors hover:bg-red-800 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-500/40 disabled:cursor-not-allowed disabled:opacity-50";

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
  // Spec 0071, A13 (spec-reviewer-Fund, 2. Runde): Die beiden Lade-Effekte
  // unten laufen genau einmal beim Mount. `t` direkt zu verwenden hätte sie
  // an die Sprachwahl gekoppelt (und damit bei jedem Sprachwechsel neu
  // geladen); ein Ref hält die jeweils aktuelle Übersetzungsfunktion, ohne
  // die Abhängigkeitsliste aufzublähen.
  const tRef = useRef(t);
  useEffect(() => {
    tRef.current = t;
  }, [t]);
  const [providers, setProviders] = useState<AiProviderConfigDto[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<AiProviderConfigInput>(emptyForm());
  const [submitting, setSubmitting] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsFailed, setModelsFailed] = useState(false);
  // Spec 0069, Teil A5: bekannter Backend-Code des letzten Discovery-
  // Fehlschlags, für den übersetzten Grund unter dem bisherigen Hinweis —
  // `null` sowohl "kein Fehlschlag" als auch "Fehlschlag ohne bekannten
  // Code" (dann bleibt nur der bisherige generische Hinweis stehen).
  const [modelsFailedCode, setModelsFailedCode] = useState<string | null>(null);
  const [credentialTestRunning, setCredentialTestRunning] = useState(false);
  const [credentialTestResult, setCredentialTestResult] =
    useState<TestAiProviderCredentialsResult | null>(null);
  const [attestationResults, setAttestationResults] = useState<Record<string, string>>({});
  const [attestationLoading, setAttestationLoading] = useState<Record<string, boolean>>({});
  const [attestationErrors, setAttestationErrors] = useState<Record<string, string>>({});
  const [riskClassifierEnabled, setRiskClassifierEnabled] = useState(false);
  const [riskClassifierProviderId, setRiskClassifierProviderId] = useState<string | null>(null);
  const [riskSettingsSaving, setRiskSettingsSaving] = useState(false);
  // Spec 0069, Teil B: Ollama-Erkennung. `providersLoaded` gate für B1
  // ("... UND die Provider-Liste geladen ist") — ohne dieses Flag würde
  // die Probe schon vor dem ersten `listAiProviders()`-Ergebnis über eine
  // noch leere `providers`-Liste fälschlich "kein Ollama-Provider"
  // schließen. `ollamaDismissed` ist reiner Komponenten-State (kein
  // Storage) — "Nein danke" blendet die Karte bis zum nächsten Öffnen der
  // Einstellungen aus (Spec B3), also bis zum nächsten Mount dieser
  // Komponente.
  const [providersLoaded, setProvidersLoaded] = useState(false);
  const [ollamaProbe, setOllamaProbe] = useState<OllamaProbeState | null>(null);
  const [ollamaDismissed, setOllamaDismissed] = useState(false);
  const [ollamaSelectedModel, setOllamaSelectedModel] = useState("");
  const [ollamaAdding, setOllamaAdding] = useState(false);

  useEffect(() => {
    loadRiskClassifierSettings()
      .then((settings) => {
        setRiskClassifierEnabled(settings.enabled);
        setRiskClassifierProviderId(settings.providerId);
      })
      .catch((err) => setError(translateErrorCode(tRef.current, commandErrorCode(err), commandErrorMessage(err))));
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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setRiskSettingsSaving(false);
    }
  };

  const reload = () => {
    listAiProviders()
      .then((list) => {
        setProviders(list);
        setProvidersLoaded(true);
      })
      .catch((err) =>
        setError(translateErrorCode(tRef.current, commandErrorCode(err), commandErrorMessage(err))),
      );
  };

  useEffect(reload, []);

  /** Spec 0069, Teil B2: über den bestehenden `discover_models`-Command,
   * kein neuer Tauri-Command, kein neuer HTTP-Pfad — Timeout/Logging
   * kommen unverändert aus `discovery.rs` (Spec 0068, Teil 5a). Die
   * übrigen Formularfelder in diesem synthetischen Konfigurationsobjekt
   * sind für `discover_models` irrelevant (das Backend liest nur
   * `providerType`/`baseUrl`/`apiKey`/`extraHeaders`), aber vom Typ
   * `AiProviderConfigInput` verlangt. */
  const runOllamaProbe = async () => {
    setOllamaProbe({ status: "loading" });
    try {
      const models = await discoverModels({
        providerType: "ollama",
        displayName: "",
        baseUrl: OLLAMA_BASE_URL,
        model: "",
        supportsNativeToolCalling: true,
        apiKey: OLLAMA_PLACEHOLDER_API_KEY,
        extraHeaders: [],
        attestationUrl: null,
        maxTokensOverride: null,
      });
      if (models.length > 0) {
        setOllamaProbe({ status: "found", models });
        setOllamaSelectedModel(models[0]);
      } else {
        setOllamaProbe({ status: "empty" });
      }
    } catch (err) {
      // Spec 0069, Teil B3: jeder andere Fehler (z. B. ein anderer Dienst
      // auf Port 11434) zeigt gar keine Karte — nicht `unreachable`, das
      // ist nur für den erwarteten "kein lokales Ollama"-Fall.
      setOllamaProbe(
        commandErrorCode(err) === "AI_LOCAL_PROVIDER_UNREACHABLE"
          ? { status: "unreachable" }
          : { status: "otherError" },
      );
    }
  };

  /** Spec 0069, Teil B1 (E1): Probe genau dann, wenn die Provider-Liste
   * geladen ist UND kein Provider vom Typ `ollama` existiert. Hängt
   * bewusst NUR von `providersLoaded` ab (nicht von `providers` selbst)
   * — sonst würde jedes spätere `reload()` (z. B. nach Hinzufügen/
   * Löschen eines *anderen* Providers) die Probe erneut auslösen, was
   * B1 als "kein Retry-Loop, kein Aufruf außerhalb dieser Komponente
   * [nach dem einen Mount-Zeitpunkt]" ausschließt. Der einzige weitere
   * Auslöser ist der "Erneut suchen"-Klick (`runOllamaProbe` direkt). */
  useEffect(() => {
    if (!providersLoaded) return;
    if (providers.some((p) => p.providerType === "ollama")) return;
    void runOllamaProbe();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providersLoaded]);

  /** Spec 0069, Teil B4 (E2): Bestätigung des Nutzers ist der Klick selbst
   * — kein automatisches Übernehmen. */
  const handleAdoptOllama = async () => {
    if (ollamaProbe?.status !== "found") return;
    setOllamaAdding(true);
    setError(null);
    try {
      const wasAnyProviderActive = providers.some((p) => p.isActive);
      const newId = await addAiProvider({
        providerType: "ollama",
        displayName: t("aiProvider.ollamaProviderName"),
        baseUrl: OLLAMA_BASE_URL,
        model: ollamaSelectedModel,
        supportsNativeToolCalling: true,
        apiKey: OLLAMA_PLACEHOLDER_API_KEY,
        extraHeaders: [],
        attestationUrl: null,
        maxTokensOverride: null,
      });
      if (!wasAnyProviderActive) {
        await setActiveAiProvider(newId);
      }
      setOllamaProbe(null);
      reload();
      onProvidersChanged();
    } catch (err) {
      // Spec 0069, Teil B4: "Fehler wie bei jedem anderen Hinzufügen" —
      // derselbe Weg wie `handleSubmit`s `catch` unten (Spec 0071:
      // `translateErrorCode` statt des rohen `Display`-Texts, z. B. für
      // `KEYCHAIN_UNAVAILABLE`), nichts aktiv gesetzt (der
      // `setActiveAiProvider`-Aufruf oben ist nie erreicht, wenn schon
      // `addAiProvider` wirft).
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setOllamaAdding(false);
    }
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      // Spec 0069, Teil B5: Ollama mit leerem Key -> Platzhalter statt des
      // (dann leeren) `form.apiKey` — jeder andere Providertyp unverändert.
      const newId = await addAiProvider({ ...form, apiKey: effectiveApiKey(form) });
      // Spec 0025, Abschnitt 4: "beim Speichern ... abrufen" — automatisch,
      // wenn eine Attestierungs-URL hinterlegt wurde.
      if (form.attestationUrl) {
        void handleFetchAttestation(newId);
      }
      setForm(emptyForm());
      setModels([]);
      setModelsFailed(false);
      setModelsFailedCode(null);
      setCredentialTestResult(null);
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
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
    setModelsFailedCode(null);
    try {
      const discovered = await discoverModels({ ...form, apiKey: effectiveApiKey(form) });
      setModels(discovered);
    } catch (err) {
      setModelsFailed(true);
      setModelsFailedCode(commandErrorCode(err));
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
      const result = await testAiProviderCredentials({ ...form, apiKey: effectiveApiKey(form) });
      setCredentialTestResult(result);
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
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
      setAttestationErrors((prev) => ({
        ...prev,
        [providerId]: translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)),
      }));
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
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    }
  };

  const handleSetActive = async (id: string) => {
    setError(null);
    try {
      await setActiveAiProvider(id);
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    }
  };

  const apiKeyWarning = apiKeyFormatWarning(form.providerType, form.baseUrl, form.apiKey);

  // Spec 0069, Teil B3: `otherError` (und `unreachable`, wenn dessen
  // Sichtbarkeits-Bedingung nicht zutrifft) zeigen "still" gar keine
  // Karte — nicht nur einen leeren Kartenrahmen. Als eigene Variable
  // statt der Bedingung direkt im JSX, damit der leere-Rahmen-Fall
  // strukturell ausgeschlossen ist (derselbe `<section>` wird nur
  // gerendert, wenn er auch Inhalt hat).
  const showOllamaCard =
    ollamaProbe !== null &&
    !ollamaDismissed &&
    (ollamaProbe.status === "loading" ||
      ollamaProbe.status === "found" ||
      ollamaProbe.status === "empty" ||
      (ollamaProbe.status === "unreachable" &&
        (providers.length === 0 || form.providerType === "ollama")));

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
                      className={SECONDARY_BUTTON_CLASS}
                    >
                      {t("aiProvider.setActive")}
                    </button>
                  )}
                  <button type="button" onClick={() => handleDelete(provider.id)} className={DANGER_BUTTON_CLASS}>
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
            <span className={LABEL_TEXT_CLASS}>{t("aiProvider.riskClassifierProvider")}</span>
            <select
              value={riskClassifierProviderId ?? ""}
              onChange={(e) => handleRiskClassifierChange(true, e.target.value || null)}
              disabled={riskSettingsSaving}
              className={`mt-1 w-full ${FIELD_CLASS}`}
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

      {/* Spec 0069, Teil B3: eine Karte oberhalb von "Provider
       * hinzufügen", je nach Probe-Ergebnis. `ollamaDismissed` blendet
       * sie für den Rest dieses Mounts komplett aus (auch einen später
       * per "Erneut suchen" neu gesetzten Zustand) — "Nein danke" heißt
       * "nicht mehr fragen, bis die Einstellungen erneut geöffnet
       * werden". */}
      {showOllamaCard && ollamaProbe && (
        <section className={`mb-6 ${CARD_CLASS}`}>
          {ollamaProbe.status === "loading" && (
            <p className="text-sm text-slate-400">{t("aiProvider.ollamaProbeSearching")}</p>
          )}

          {ollamaProbe.status === "found" && (
            <div className="space-y-2">
              <p className="text-sm font-medium text-slate-100">
                {t("aiProvider.ollamaProbeFoundHeading")}
              </p>
              <label className={LABEL_CLASS}>
                <span className={LABEL_TEXT_CLASS}>{t("aiProvider.model")}</span>
                <select
                  value={ollamaSelectedModel}
                  onChange={(e) => setOllamaSelectedModel(e.target.value)}
                  className={`mt-1 w-full ${FIELD_CLASS}`}
                >
                  {ollamaProbe.models.map((model) => (
                    <option key={model} value={model}>
                      {model}
                    </option>
                  ))}
                </select>
              </label>
              <p className="text-xs text-slate-500">
                {providers.some((p) => p.isActive)
                  ? t("aiProvider.ollamaProbeFoundHintInactive")
                  : t("aiProvider.ollamaProbeFoundHintWillActivate")}
              </p>
              <div className="flex items-center gap-3">
                <button
                  type="button"
                  onClick={handleAdoptOllama}
                  disabled={ollamaAdding}
                  className={SECONDARY_BUTTON_CLASS}
                >
                  {ollamaAdding ? t("aiProvider.adding") : t("aiProvider.ollamaProbeAdopt")}
                </button>
                <button
                  type="button"
                  onClick={() => setOllamaDismissed(true)}
                  className="text-xs text-slate-400 underline hover:text-slate-200"
                >
                  {t("aiProvider.ollamaProbeDismiss")}
                </button>
              </div>
            </div>
          )}

          {ollamaProbe.status === "empty" && (
            <div className="space-y-2">
              <p className="text-sm font-medium text-slate-100">
                {t("aiProvider.ollamaProbeEmptyHeading")}
              </p>
              <p className="text-xs text-slate-500">{t("aiProvider.ollamaProbePullHint")}</p>
              <button type="button" onClick={runOllamaProbe} className={SECONDARY_BUTTON_CLASS}>
                {t("aiProvider.ollamaProbeRetry")}
              </button>
            </div>
          )}

          {/* Spec 0069, Teil B3: die "nicht gefunden"-Anleitung nur, wenn
           * gar kein Provider konfiguriert ist oder im Formular der Typ
           * Ollama gewählt ist — wer bereits z. B. Anthropic nutzt,
           * bekommt keinen Ollama-Hinweis aufgedrängt. */}
          {ollamaProbe.status === "unreachable" &&
            (providers.length === 0 || form.providerType === "ollama") && (
              <div className="space-y-2">
                <p className="text-sm font-medium text-slate-100">
                  {t("aiProvider.ollamaProbeNotFoundHeading")}
                </p>
                <p className="text-xs text-slate-500">
                  {t("aiProvider.ollamaProbeNotFoundInstructions")}
                </p>
                <button type="button" onClick={runOllamaProbe} className={SECONDARY_BUTTON_CLASS}>
                  {t("aiProvider.ollamaProbeRetry")}
                </button>
              </div>
            )}

          {/* `otherError` (und `unreachable`, wenn die Sichtbarkeits-
           * Bedingung oben nicht zutrifft): bewusst keine Karte (Spec
           * 0069 §3.B3, "still"). */}
        </section>
      )}

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
            <span className={LABEL_TEXT_CLASS}>{t("aiProvider.type")}</span>
            <select
              value={form.providerType}
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
              }}
              className={`mt-1 w-full ${FIELD_CLASS}`}
            >
              {PROVIDER_TYPES.map((type) => (
                <option key={type} value={type}>
                  {PROVIDER_TYPE_LABELS[type]}
                </option>
              ))}
            </select>
          </label>

          <label className={LABEL_CLASS}>
            <span className={LABEL_TEXT_CLASS}>{t("aiProvider.providerName")}</span>
            <input
              type="text"
              required
              value={form.displayName}
              onChange={(e) => setForm({ ...form, displayName: e.target.value })}
              className={`mt-1 w-full ${FIELD_CLASS}`}
            />
          </label>

          <label className={LABEL_CLASS}>
            <span className={LABEL_TEXT_CLASS}>{t("aiProvider.model")}</span>
            <div className="mt-1 flex gap-2">
              <input
                type="text"
                required
                list={supportsModelDiscovery(form.providerType) ? MODEL_DATALIST_ID : undefined}
                placeholder={t("aiProvider.modelPlaceholder")}
                value={form.model}
                onChange={(e) => setForm({ ...form, model: e.target.value })}
                className={`w-full ${FIELD_CLASS}`}
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
              <>
                <p className="mt-1 text-xs text-slate-500">{t("aiProvider.modelDiscoveryFailedHint")}</p>
                {/* Spec 0069, Teil A5: zusätzlich der übersetzte Grund,
                 * wenn der Fehler einen bekannten Code trägt (z. B.
                 * AI_MODEL_NOT_FOUND, AI_LOCAL_PROVIDER_UNREACHABLE) —
                 * unter dem bisherigen, unveränderten Hinweis, nicht
                 * anstelle davon. `translateErrorCode` fällt bei
                 * unbekanntem Code auf den leeren String zurück, den wir
                 * hier explizit unterdrücken statt eine leere Box zu
                 * zeigen. */}
                {modelsFailedCode && translateErrorCode(t, modelsFailedCode, "") && (
                  <p className="mt-0.5 text-xs text-slate-500">
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
                type="text"
                required
                placeholder={t("aiProvider.baseUrlPlaceholder")}
                value={form.baseUrl ?? ""}
                onChange={(e) => setForm({ ...form, baseUrl: e.target.value })}
                className={`mt-1 w-full ${FIELD_CLASS}`}
              />
            </label>
          )}

          <label className={LABEL_CLASS}>
            <span className={LABEL_TEXT_CLASS}>
              {t("aiProvider.apiKey")}
              {/* Spec 0069, Teil B5: Ollama braucht keinen Key. */}
              {form.providerType === "ollama" && (
                <span className="ml-1 font-normal text-slate-500">
                  {t("aiProvider.apiKeyNotNeededForOllama")}
                </span>
              )}
            </span>
            <input
              type="password"
              required={form.providerType !== "ollama"}
              value={form.apiKey}
              onChange={(e) => {
                setForm({ ...form, apiKey: e.target.value });
                // Spec 0050, Teil 3: ein Testergebnis bezieht sich auf den
                // Stand zum Testzeitpunkt — ändert sich der Key danach,
                // wäre ein weiterhin angezeigtes "gültig" irreführend.
                setCredentialTestResult(null);
              }}
              className={`mt-1 w-full ${FIELD_CLASS}`}
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
                // Spec 0069, Teil B5: bei Ollama ist ein leerer Key kein
                // Grund zu sperren — "Zugangsdaten testen" ist ohne Key
                // freigegeben.
                (form.providerType !== "ollama" && !form.apiKey.trim()) ||
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
                          className={`w-1/2 text-sm ${FIELD_CLASS}`}
                        />
                        <input
                          type="text"
                          placeholder={t("aiProvider.extraHeaderValuePlaceholder")}
                          value={value}
                          onChange={(e) => updateExtraHeader(index, key, e.target.value)}
                          className={`w-1/2 text-sm ${FIELD_CLASS}`}
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
                  <span className={LABEL_TEXT_CLASS}>{t("aiProvider.attestationUrlLabel")}</span>
                  <input
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
                  <p className="mt-0.5 text-xs text-slate-500">
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

        <button
          type="submit"
          disabled={submitting || (form.maxTokensOverride !== null && form.maxTokensOverride <= 0)}
          className="w-full rounded bg-indigo-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/60 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {submitting ? t("aiProvider.adding") : t("aiProvider.add")}
        </button>
      </form>
    </div>
  );
}
