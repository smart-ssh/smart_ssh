import { settingsStore } from "./i18n";

/** Spec 0026, Abschnitt 3, Punkt 1: eigene Einstellung über
 * `tauri-plugin-store` (Spec 0024-Muster) im selben Store wie die
 * UI-Sprache — keine neue SQLite-Tabelle für eine reine, nicht
 * sicherheitskritische Komfort-Einstellung. Der Rust-seitige Reader (s.
 * `crate::risk_second_opinion::resolve_second_opinion_provider`) nutzt
 * dieselben Schlüssel-Namen. */
const ENABLED_KEY = "riskClassifierEnabled";
const PROVIDER_ID_KEY = "riskClassifierProviderId";

export interface RiskClassifierSettings {
  enabled: boolean;
  providerId: string | null;
}

export async function loadRiskClassifierSettings(): Promise<RiskClassifierSettings> {
  const store = await settingsStore();
  const enabled = (await store.get<boolean>(ENABLED_KEY)) ?? false;
  const providerId = (await store.get<string>(PROVIDER_ID_KEY)) ?? null;
  return { enabled, providerId };
}

/** Spec 0026, Abschnitt 3, Punkt 1: erst bei der nächsten `connect()`
 * wirksam (s. `Session::risk_second_opinion_provider`-Doc-Kommentar im
 * Backend) — kein "Wirkung sofort" wie bei der Sprache (Spec 0024), das
 * hier absichtlich anders gehandhabt wird, da eine laufende Session einen
 * bereits aufgebauten `AiProvider` fest referenziert. */
export async function saveRiskClassifierSettings(settings: RiskClassifierSettings): Promise<void> {
  const store = await settingsStore();
  await store.set(ENABLED_KEY, settings.enabled);
  await store.set(PROVIDER_ID_KEY, settings.providerId);
  await store.save();
}
