import { settingsStore } from "./i18n";

/** Issue #356, Spec 0107 §8: Einstellung „Automatisch neu verbinden“ im
 * selben `tauri-plugin-store` wie die übrigen UI-Einstellungen. Das Backend
 * liest denselben Schlüssel bei jedem Verbindungsabbruch
 * (`app_logic::auto_reconnect::SETTING_KEY`), eine Änderung gilt daher ab
 * dem nächsten Abbruch. Gespeichert wird die Dauer in Sekunden, `0` = aus. */
const KEY = "autoReconnectSeconds";

export const AUTO_RECONNECT_SECONDS = [0, 30, 60, 300] as const;
export type AutoReconnectSeconds = (typeof AUTO_RECONNECT_SECONDS)[number];
export const DEFAULT_AUTO_RECONNECT_SECONDS: AutoReconnectSeconds = 60;

/** Dieselbe Auswertung wie im Backend: nur die vier angebotenen Werte
 * gelten, alles andere (fehlend, kaputt, von Hand editiert) ist der
 * Standard. */
export function parseAutoReconnectSeconds(value: unknown): AutoReconnectSeconds {
  return (AUTO_RECONNECT_SECONDS as readonly unknown[]).includes(value)
    ? (value as AutoReconnectSeconds)
    : DEFAULT_AUTO_RECONNECT_SECONDS;
}

export async function loadAutoReconnectSeconds(): Promise<AutoReconnectSeconds> {
  const store = await settingsStore();
  return parseAutoReconnectSeconds(await store.get<unknown>(KEY));
}

export async function saveAutoReconnectSeconds(value: AutoReconnectSeconds): Promise<void> {
  const store = await settingsStore();
  await store.set(KEY, value);
  await store.save();
}
