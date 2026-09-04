/** Zentrale Anlaufstelle für `FeatureLocked`-Fehler (Spec 0038, Abschnitt
 * 4): `api.ts`s `invokeCommand`-Wrapper meldet jeden abgelehnten Aufruf mit
 * `feature_locked`-Payload hier — unabhängig davon, welcher der ca. 70
 * `invoke`-Aufrufstellen ihn ausgelöst hat. Ein einzelner
 * `<FeatureLockedDialog />` (in `App.tsx` einmalig gemountet) abonniert
 * diesen Bus und zeigt den Hinweis ("Diese Funktion erfordert Pro") —
 * einzelne Komponenten müssen `feature_locked` nicht selbst auswerten. */

import type { FeatureLockedPayload } from "./entitlements";

type Listener = (payload: FeatureLockedPayload) => void;

const listeners = new Set<Listener>();

export function publishFeatureLocked(payload: FeatureLockedPayload): void {
  for (const listener of listeners) listener(payload);
}

/** Gibt eine Unsubscribe-Funktion zurück (React-`useEffect`-Cleanup-Form). */
export function subscribeFeatureLocked(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
