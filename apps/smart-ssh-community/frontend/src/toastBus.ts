/** Spec 0067, Teil B: app-weite Erfolgs-/Fehlermeldungen. Dasselbe
 * Bus-Muster wie `navigationBus.ts` — eine zentrale Anlaufstelle statt
 * Props quer durch den Baum; angezeigt von `ToastHost` an der App-Wurzel. */

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface ToastInput {
  kind: "success" | "error";
  message: string;
  action?: ToastAction;
}

export interface Toast extends ToastInput {
  id: number;
}

type Listener = (toast: Toast) => void;

const listeners = new Set<Listener>();
let nextId = 1;

export function showToast(input: ToastInput): void {
  const toast = { ...input, id: nextId++ };
  for (const listener of listeners) listener(toast);
}

/** Gibt eine Unsubscribe-Funktion zurück (React-`useEffect`-Cleanup-Form). */
export function subscribeToasts(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
