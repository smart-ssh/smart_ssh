/** Issue #12 / ADR 0104: `HostKeyPromptHost` (an der `App`-Wurzel, stets
 * gemountet) besitzt den einzigen Listener auf
 * `host-key-verification-needed`. Endet ein `connect()` aus `ServerList`
 * (Erfolg oder Fehler), schließt `ServerList` den evtl. noch offenen
 * Host-Key-Dialog über dieses Signal — dasselbe Bus-Muster wie
 * `toastBus.ts`, statt Props quer durch `MainScreen` zu reichen. */

import { createBus } from "./createBus";

type Listener = () => void;

const bus = createBus<void>();

export function clearHostKeyPrompt(): void {
  bus.publish();
}

/** Gibt eine Unsubscribe-Funktion zurück (React-`useEffect`-Cleanup-Form). */
export function subscribeHostKeyPromptClear(listener: Listener): () => void {
  return bus.subscribe(listener);
}
