import { settingsStore } from "./i18n";

/** Spec 0026, Abschnitt 3, Punkt 1: eigene Einstellung über
 * `tauri-plugin-store` (Spec 0024-Muster) im selben Store wie die
 * UI-Sprache — keine neue SQLite-Tabelle für eine reine, nicht
 * sicherheitskritische Komfort-Einstellung. Der Rust-seitige Reader (s.
 * `crate::risk_second_opinion::resolve_second_opinion_provider`) nutzt
 * dieselben Schlüssel-Namen. */
const ENABLED_KEY = "riskClassifierEnabled";
const PROVIDER_ID_KEY = "riskClassifierProviderId";
/** Spec 0092, A1.1 — derselbe Schlüssel wie im Backend
 * (`app_shell::risk_second_opinion::RED_RISK_ALWAYS_CONFIRM_KEY`). */
const RED_RISK_ALWAYS_CONFIRM_KEY = "redRiskAlwaysConfirm";

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

/** Spec 0092, A1.2/A1.4: liest die Einstellung „Bei rotem Risiko immer
 * nachfragen" — **derselbe Fail-safe wie im Backend**
 * (`app_shell::risk_second_opinion::red_risk_always_confirm_from_stored`):
 * fehlt der Schlüssel oder ist der gespeicherte Wert kein boolescher Wert
 * (z. B. ein String `"false"`), gilt „an". Nur ein tatsächliches JSON
 * `false` schaltet ab. Frontend und Backend lesen denselben Schlüssel
 * unabhängig voneinander — dieselbe Auswertung ist hier bewusst dupliziert
 * (kein gemeinsamer Code zwischen Rust und TypeScript), U3 sichert die
 * Übereinstimmung ab. */
export async function loadRedRiskAlwaysConfirm(): Promise<boolean> {
  const store = await settingsStore();
  const value = await store.get<unknown>(RED_RISK_ALWAYS_CONFIRM_KEY);
  return typeof value === "boolean" ? value : true;
}

/** Spec 0092, A1.3: wie bei der Zweitmeinung wirkt eine Änderung erst ab
 * der nächsten Verbindung (das Backend liest den Wert einmalig in
 * `connect()`) — trotzdem sofort gespeichert, damit nichts verloren geht. */
export async function saveRedRiskAlwaysConfirm(value: boolean): Promise<void> {
  const store = await settingsStore();
  await store.set(RED_RISK_ALWAYS_CONFIRM_KEY, value);
  await store.save();
}
