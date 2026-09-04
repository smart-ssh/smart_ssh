/** Einfacher Hinweis-Dialog für `FeatureLocked`-Fehler (Spec 0038,
 * Abschnitt 4) — kein vollständiges Upgrade-Flow-Design in diesem Schritt
 * (folgt, sobald es mehr als ein gegatetes Feature gibt), nur die
 * Infrastruktur: einmalig in `App.tsx` gemountet, abonniert
 * `featureLockedBus` und zeigt den zuletzt gemeldeten Sperr-Grund an.
 *
 * Bewusst ohne `react-i18next`-Anbindung: dieses Paket lebt außerhalb von
 * `apps/smart-ssh-community/frontend` und kennt dessen `i18n`-Setup nicht
 * (Spec 0038 zielt auf eine von der konkreten App entkoppelte
 * Registry-Infrastruktur). Fest deutscher Text ist hier ein bewusster,
 * eng begrenzter Kompromiss, kein Vorgriff auf das noch ausstehende
 * vollständige Upgrade-Flow-Design (Spec 0038, Abschnitt 4/9). */

import { useEffect, useState } from "react";

import type { FeatureLockedPayload } from "./entitlements";
import { subscribeFeatureLocked } from "./featureLockedBus";

export function FeatureLockedDialog() {
  const [locked, setLocked] = useState<FeatureLockedPayload | null>(null);

  useEffect(() => subscribeFeatureLocked(setLocked), []);

  if (!locked) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="w-full max-w-sm space-y-3 rounded border border-slate-700 bg-slate-900 p-5 text-slate-100">
        <h2 className="font-heading text-sm font-semibold tracking-wide">
          Diese Funktion erfordert Pro
        </h2>
        <p className="text-sm text-slate-300">
          {locked.feature} ist in deiner aktuellen Edition ({locked.tier}) nicht
          verfügbar.
        </p>
        <button
          type="button"
          onClick={() => setLocked(null)}
          className="w-full rounded border border-slate-600 bg-slate-800 px-3 py-1.5 text-sm font-semibold hover:bg-slate-700"
        >
          Schließen
        </button>
      </div>
    </div>
  );
}
