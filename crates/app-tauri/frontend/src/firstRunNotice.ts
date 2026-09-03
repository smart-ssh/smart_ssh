import { settingsStore } from "./i18n";

/** Spec 0031, Abschnitt 2: derselbe `tauri-plugin-store`-Ablageort/Muster
 * wie die UI-Sprache (Spec 0024) und die Risiko-Zweitmeinungs-Einstellung
 * (`riskSettings.ts`) — der Rust-seitige Reader (s.
 * `crate::first_run_notice::is_acknowledged`) nutzt denselben Schlüssel. */
const ACKNOWLEDGED_KEY = "first_run_notice_acknowledged";

export async function loadFirstRunNoticeAcknowledged(): Promise<boolean> {
  const store = await settingsStore();
  return (await store.get<boolean>(ACKNOWLEDGED_KEY)) ?? false;
}

/** Spec 0031, Abschnitt 4: nach Bestätigung dauerhaft gesetzt, erscheint bei
 * künftigen Starts nicht mehr. Die eigentliche Durchsetzung (Blockieren von
 * `connect()`) passiert serverseitig (s. `crate::commands::connect_session`)
 * — dieser Aufruf hier ist nur der UI-seitige Teil, der den Screen selbst
 * nicht mehr zeigt. */
export async function saveFirstRunNoticeAcknowledged(): Promise<void> {
  const store = await settingsStore();
  await store.set(ACKNOWLEDGED_KEY, true);
  await store.save();
}
