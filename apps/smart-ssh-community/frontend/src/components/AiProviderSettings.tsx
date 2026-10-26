import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  addAiProvider,
  commandErrorCode,
  commandErrorMessage,
  deleteAiProvider,
  discoverModels,
  fetchAttestationInfo,
  listAiProviders,
  setActiveAiProvider,
  updateAiProvider,
} from "../api";
import { Button } from "./ui";
import { AddAiProviderDialog, type AddAiProviderResult } from "./AddAiProviderDialog";
import { CARD_CLASS, FIELD_CLASS, LABEL_CLASS, LABEL_TEXT_CLASS } from "./aiProviderFormStyles";
import { translateErrorCode } from "../errorCodes";
import { OLLAMA_BASE_URL, OLLAMA_PLACEHOLDER_API_KEY } from "../ollama";
import {
  loadRedRiskAlwaysConfirm,
  loadRiskClassifierSettings,
  saveRedRiskAlwaysConfirm,
  saveRiskClassifierSettings,
} from "../riskSettings";
import {
  type AiProviderConfigDto,
  supportsWebResearch,
} from "../types";

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
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [editingProvider, setEditingProvider] = useState<AiProviderConfigDto | null>(null);
  const addButtonRef = useRef<HTMLButtonElement>(null);
  const [attestationResults, setAttestationResults] = useState<Record<string, string>>({});
  const [attestationLoading, setAttestationLoading] = useState<Record<string, boolean>>({});
  const [attestationErrors, setAttestationErrors] = useState<Record<string, string>>({});
  const [riskClassifierEnabled, setRiskClassifierEnabled] = useState(false);
  const [riskClassifierProviderId, setRiskClassifierProviderId] = useState<string | null>(null);
  const [riskSettingsSaving, setRiskSettingsSaving] = useState(false);
  /** Spec 0092, A1.4: unabhängig von `riskClassifierEnabled` bedienbar —
   * eigener State, eigene Ladung, eigener Speicherpfad. Fail-safe-Default
   * `true`, bis geladen (deckt sich mit A1.2, s. `riskSettings.ts`). */
  const [redRiskAlwaysConfirm, setRedRiskAlwaysConfirm] = useState(true);
  const [redRiskSettingSaving, setRedRiskSettingSaving] = useState(false);
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

  // Spec 0092, A1.4: eigener Lade-Effekt, unabhängig vom Zweitmeinungs-Block
  // oben — der Schalter ist bedienbar, auch wenn die Zweitmeinung nie
  // geladen/aktiviert wird.
  useEffect(() => {
    loadRedRiskAlwaysConfirm()
      .then(setRedRiskAlwaysConfirm)
      .catch((err) => setError(translateErrorCode(tRef.current, commandErrorCode(err), commandErrorMessage(err))));
  }, []);

  /** Spec 0092, A1.3: erst bei der nächsten `connect()` wirksam — trotzdem
   * sofort gespeichert (dasselbe Muster wie `handleRiskClassifierChange`
   * oben). */
  const handleRedRiskAlwaysConfirmChange = async (value: boolean) => {
    setRedRiskAlwaysConfirm(value);
    setRedRiskSettingSaving(true);
    try {
      await saveRedRiskAlwaysConfirm(value);
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    } finally {
      setRedRiskSettingSaving(false);
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
        webResearchEnabled: true,
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
        webResearchEnabled: true,
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

  /** Issue #603: the dialog saved the provider (and tried to activate it). */
  const handleAdded = ({ newId, fetchAttestation, activationError }: AddAiProviderResult) => {
    setAddDialogOpen(false);
    setError(null);
    // Spec 0025, Abschnitt 4: "beim Speichern ... abrufen" - automatisch,
    // wenn eine Attestierungs-URL hinterlegt wurde.
    if (fetchAttestation) {
      void handleFetchAttestation(newId);
    }
    reload();
    onProvidersChanged();
    if (activationError !== null) {
      setError(
        translateErrorCode(t, commandErrorCode(activationError), commandErrorMessage(activationError)),
      );
    }
  };

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

  /** Spec 0105: Web-Recherche eines bestehenden Providers ein-/ausschalten.
   * Leerer `apiKey` = Zugangsdaten unverändert (s. `update_ai_provider`). */
  const handleWebResearchChange = async (provider: AiProviderConfigDto, enabled: boolean) => {
    setError(null);
    try {
      await updateAiProvider(provider.id, {
        providerType: provider.providerType,
        displayName: provider.displayName,
        baseUrl: provider.baseUrl,
        model: provider.model,
        supportsNativeToolCalling: provider.supportsNativeToolCalling,
        apiKey: "",
        extraHeaders: provider.extraHeaders,
        attestationUrl: provider.attestationUrl,
        maxTokensOverride: provider.maxTokensOverride,
        webResearchEnabled: enabled,
      });
      reload();
      onProvidersChanged();
    } catch (err) {
      setError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err)));
    }
  };

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
        providers.length === 0));

  return (
    <div>
      {error && (
        <p className="mb-4 rounded bg-danger-subtle px-3 py-2 text-sm text-danger-fg-soft">{error}</p>
      )}

      {/* Spec 0056, Teil 3: eigene Karte statt freistehender Liste — macht
       * sichtbar, dass dies ein abgeschlossener Bereich ("was ist bereits
       * konfiguriert") ist, getrennt vom Formular darunter ("neuen Provider
       * anlegen"). Die Liste selbst behält ihre bisherige `divide-y`-
       * Struktur, bekommt aber einen dunkleren Innenhintergrund
       * (`slate-950/40`) als die Karte (`slate-900/40`), damit sich Karte
       * und Listeninhalt trotz gleicher Rahmenfarbe voneinander abheben. */}
      <section className={`mb-6 ${CARD_CLASS}`}>
        <h3 className="font-heading mb-3 text-sm font-semibold tracking-wide text-fg-soft">
          {t("aiProvider.configuredProvidersTitle")}
        </h3>
        <ul className="divide-y divide-border rounded-md border border-border bg-surface-sunken/40">
          {providers.length === 0 && (
            <li className="px-4 py-3 text-sm text-fg-subtle">{t("aiProvider.noProviders")}</li>
          )}
          {providers.map((provider) => (
            <li key={provider.id} className="px-4 py-3">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <p className="flex items-center font-medium text-fg">
                    <span className="truncate" title={provider.displayName}>
                      {provider.displayName}
                    </span>
                    {provider.isActive && (
                      <span className="ml-2 shrink-0 rounded bg-success-subtle-hover px-2 py-0.5 text-xs text-success-fg-soft">
                        {t("aiProvider.active")}
                      </span>
                    )}
                  </p>
                  <p
                    className="truncate text-sm text-fg-subtle"
                    title={`${t(`aiProvider.providerTypes.${provider.providerType}`)} · ${provider.model}`}
                  >
                    {t(`aiProvider.providerTypes.${provider.providerType}`)} · {provider.model}
                  </p>
                </div>
                <div className="flex shrink-0 gap-2">
                  {!provider.isActive && (
                    <Button size="sm"
                      onClick={() => handleSetActive(provider.id)}
                      
                    >
                      {t("aiProvider.setActive")}
                    </Button>
                  )}
                  <Button size="sm" onClick={() => setEditingProvider(provider)}>
                    {t("common.edit")}
                  </Button>
                  <Button size="sm" variant="danger" onClick={() => handleDelete(provider.id)} >
                    {t("common.delete")}
                  </Button>
                </div>
              </div>

              {supportsWebResearch(provider.providerType, provider.baseUrl) && (
                <label className="mt-2 flex items-center gap-2 text-sm text-fg-muted">
                  <input
                    type="checkbox"
                    checked={provider.webResearchEnabled}
                    onChange={(e) => handleWebResearchChange(provider, e.target.checked)}
                  />
                  {t("aiProvider.webResearch")}
                </label>
              )}

              {provider.attestationUrl && (
                <div className="mt-2 space-y-1.5">
                  <Button size="sm"
                    onClick={() => handleFetchAttestation(provider.id)}
                    disabled={attestationLoading[provider.id]}
                    
                  >
                    {attestationLoading[provider.id]
                      ? t("aiProvider.attestationFetching")
                      : t("aiProvider.attestationFetch")}
                  </Button>
                  {attestationErrors[provider.id] && (
                    <p className="text-xs text-danger-fg">{attestationErrors[provider.id]}</p>
                  )}
                  {attestationResults[provider.id] && (
                    <div className="space-y-1">
                      <p className="text-xs text-warning-fg-soft">
                        {t("aiProvider.attestationDisclaimerBeforeNot")}
                        <strong>{t("aiProvider.attestationDisclaimerNot")}</strong>
                        {t("aiProvider.attestationDisclaimerAfterNot")}
                      </p>
                      <p className="text-xs font-semibold text-fg-subtle">
                        {t("aiProvider.attestationResultLabel")}
                      </p>
                      <pre className="max-h-40 overflow-auto rounded border border-border bg-surface-sunken p-2 text-xs whitespace-pre-wrap text-fg-muted">
                        {attestationResults[provider.id]}
                      </pre>
                    </div>
                  )}
                </div>
              )}
            </li>
          ))}
        </ul>
        <Button
          ref={addButtonRef}
          variant={providers.length === 0 ? "primary" : "secondary"}
          size="sm"
          className="mt-3"
          onClick={() => setAddDialogOpen(true)}
        >
          {t("aiProvider.addProvider")}
        </Button>
      </section>

      {/* Spec 0026, Abschnitt 3, Punkt 1: eigener Abschnitt für die
       * optionale KI-Zweitmeinung zur Daten-Risiko-Achse — standardmäßig
       * deaktiviert (Opt-in), separat wählbarer Provider, Hinweis auf ein
       * empfohlenes lokales Modell. Spec 0056, Teil 3: eigene Karte statt
       * einer bloßen oberen Trennlinie — bislang lief dieser Bereich
       * optisch nahtlos in die Provider-Liste über. */}
      <section className={`mb-6 ${CARD_CLASS}`}>
        <h3 className="font-heading mb-2 text-sm font-semibold tracking-wide text-fg-soft">
          {t("aiProvider.riskClassifierTitle")}
        </h3>
        <p className="mb-2 text-xs text-fg-subtle">{t("aiProvider.riskClassifierHint")}</p>
        <label className="mb-2 flex items-center gap-2 text-sm text-fg-muted">
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

      {/* Spec 0092, A1.4: eigener Abschnitt, unabhängig von der
       * KI-Zweitmeinung oben bedienbar — bewusst eine eigene Karte statt
       * eines weiteren Feldes in der Zweitmeinungs-Karte, weil die
       * Einstellung ohne aktivierte Zweitmeinung genauso wirkt
       * (regelbasiertes Rot). */}
      <section className={`mb-6 ${CARD_CLASS}`}>
        <h3 className="font-heading mb-2 text-sm font-semibold tracking-wide text-fg-soft">
          {t("aiProvider.redRiskAlwaysConfirmTitle")}
        </h3>
        <p className="mb-2 text-xs text-fg-subtle">{t("aiProvider.redRiskAlwaysConfirmHint")}</p>
        <label className="mb-2 flex items-center gap-2 text-sm text-fg-muted">
          <input
            type="checkbox"
            checked={redRiskAlwaysConfirm}
            disabled={redRiskSettingSaving}
            onChange={(e) => handleRedRiskAlwaysConfirmChange(e.target.checked)}
          />
          {t("aiProvider.redRiskAlwaysConfirmEnable")}
        </label>
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
            <p className="text-sm text-fg-subtle">{t("aiProvider.ollamaProbeSearching")}</p>
          )}

          {ollamaProbe.status === "found" && (
            <div className="space-y-2">
              <p className="text-sm font-medium text-fg">
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
              <p className="text-xs text-fg-subtle">
                {providers.some((p) => p.isActive)
                  ? t("aiProvider.ollamaProbeFoundHintInactive")
                  : t("aiProvider.ollamaProbeFoundHintWillActivate")}
              </p>
              <div className="flex items-center gap-3">
                <Button size="sm"
                  onClick={handleAdoptOllama}
                  disabled={ollamaAdding}
                  
                >
                  {ollamaAdding ? t("aiProvider.adding") : t("aiProvider.ollamaProbeAdopt")}
                </Button>
                <Button variant="ghost" size="sm" onClick={() => setOllamaDismissed(true)}>
                  {t("aiProvider.ollamaProbeDismiss")}
                </Button>
              </div>
            </div>
          )}

          {ollamaProbe.status === "empty" && (
            <div className="space-y-2">
              <p className="text-sm font-medium text-fg">
                {t("aiProvider.ollamaProbeEmptyHeading")}
              </p>
              <p className="text-xs text-fg-subtle">{t("aiProvider.ollamaProbePullHint")}</p>
              <Button size="sm" onClick={runOllamaProbe} >
                {t("aiProvider.ollamaProbeRetry")}
              </Button>
            </div>
          )}

          {/* Spec 0069, Teil B3: die "nicht gefunden"-Anleitung nur, wenn
           * gar kein Provider konfiguriert ist oder im Formular der Typ
           * Ollama gewählt ist — wer bereits z. B. Anthropic nutzt,
           * bekommt keinen Ollama-Hinweis aufgedrängt. */}
          {ollamaProbe.status === "unreachable" &&
            providers.length === 0 && (
              <div className="space-y-2">
                <p className="text-sm font-medium text-fg">
                  {t("aiProvider.ollamaProbeNotFoundHeading")}
                </p>
                <p className="text-xs text-fg-subtle">
                  {t("aiProvider.ollamaProbeNotFoundInstructions")}
                </p>
                <Button size="sm" onClick={runOllamaProbe} >
                  {t("aiProvider.ollamaProbeRetry")}
                </Button>
              </div>
            )}

          {/* `otherError` (und `unreachable`, wenn die Sichtbarkeits-
           * Bedingung oben nicht zutrifft): bewusst keine Karte (Spec
           * 0069 §3.B3, "still"). */}
        </section>
      )}

      {editingProvider && (
        <AddAiProviderDialog
          key={editingProvider.id}
          provider={editingProvider}
          willActivate={false}
          onClose={() => setEditingProvider(null)}
          onAdded={() => undefined}
          onUpdated={() => {
            setEditingProvider(null);
            setError(null);
            reload();
            onProvidersChanged();
          }}
        />
      )}

      {addDialogOpen && (
        <AddAiProviderDialog
          willActivate={providersLoaded && !providers.some((p) => p.isActive)}
          onClose={() => setAddDialogOpen(false)}
          onAdded={handleAdded}
        />
      )}
    </div>
  );
}
