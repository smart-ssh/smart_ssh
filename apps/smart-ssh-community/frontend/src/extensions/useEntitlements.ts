/** Spec 0038, Abschnitt 4: liest den Entitlement-Stand per `get_entitlements`
 * einmalig beim Mount, abonniert danach `entitlements:changed`
 * (`crates/app-shell/src/lib.rs`s `run()`-Setup-Task) für spätere
 * Änderungen. Für `FixedEntitlements` (Community Edition, aktuell die
 * einzige) feuert das Event nie — der Hook liefert dann dauerhaft den beim
 * Start gelesenen Stand, was korrekt ist (er ändert sich ja tatsächlich
 * nie). */

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { Entitlements } from "./entitlements";

export interface UseEntitlementsResult {
  entitlements: Entitlements | null;
  loading: boolean;
}

export function useEntitlements(): UseEntitlementsResult {
  const [entitlements, setEntitlements] = useState<Entitlements | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    invoke<Entitlements>("get_entitlements")
      .then((value) => {
        if (!cancelled) setEntitlements(value);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    const unlistenPromise = listen<Entitlements>("entitlements:changed", (event) => {
      if (!cancelled) setEntitlements(event.payload);
    });

    return () => {
      cancelled = true;
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  return { entitlements, loading };
}
