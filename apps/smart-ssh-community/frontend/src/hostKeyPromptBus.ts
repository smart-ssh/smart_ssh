/** Issue #12 / ADR 0104: `HostKeyPromptHost` (an der `App`-Wurzel, stets
 * gemountet) besitzt den einzigen Listener auf
 * `host-key-verification-needed`. Endet ein `connect()` aus `ServerList`
 * (Erfolg oder Fehler), schließt `ServerList` den evtl. noch offenen
 * Host-Key-Dialog über dieses Signal — dasselbe Bus-Muster wie
 * `toastBus.ts`, statt Props quer durch `MainScreen` zu reichen. */

type Listener = () => void;

const listeners = new Set<Listener>();

export function clearHostKeyPrompt(): void {
  for (const listener of listeners) listener();
}

/** Gibt eine Unsubscribe-Funktion zurück (React-`useEffect`-Cleanup-Form). */
export function subscribeHostKeyPromptClear(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
